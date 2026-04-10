//! 进程与线程管理模块
//!
//! ## 与第七章的区别
//!
//! 第七章中 `Process` 既是资源容器又是执行单元。
//! 第八章将两者分离：
//! - **Process**：资源容器，管理地址空间、文件描述符、**同步原语列表**、信号
//! - **Thread**：执行单元，管理 TID 和上下文
//!
//! 同一进程的所有线程共享 `Process` 中的资源。
//!
//! ## 新增字段
//!
//! | 字段 | 说明 |
//! |------|------|
//! | `semaphore_list` | 信号量列表（进程内所有线程共享） |
//! | `mutex_list` | 互斥锁列表 |
//! | `condvar_list` | 条件变量列表 |
//!
//! 教程阅读建议：
//!
//! - 先看 `Process` 与 `Thread` 的字段分工：明确“资源归进程、执行归线程”；
//! - 再看 `fork/exec/from_elf`：理解跨线程模型后，进程复制与替换语义如何变化；
//! - 最后结合 `processor.rs` 看线程生命周期与进程资源回收的关系。

use crate::{Sv39, Sv39Manager, build_flags, fs::Fd, map_portal, parse_flags};
use alloc::{
    alloc::alloc_zeroed,
    boxed::Box,
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    vec::Vec,
};
use core::alloc::Layout;
use spin::Mutex;
use tg_kernel_context::{LocalContext, foreign::ForeignContext};
use tg_kernel_vm::{
    AddressSpace,
    page_table::{MmuMeta, PPN, VAddr, VPN},
};
use tg_signal::Signal;
use tg_signal_impl::SignalImpl;
use tg_sync::{Condvar, Mutex as MutexTrait, Semaphore};
use tg_task_manage::{ProcId, ThreadId};
use xmas_elf::{
    ElfFile,
    header::{self, HeaderPt2, Machine},
    program,
};

/// 信号量死锁检测状态。
pub struct SemaphoreDeadlockState {
    totals: Vec<usize>,
    allocations: BTreeMap<ThreadId, BTreeMap<usize, usize>>,
    waiting: BTreeMap<ThreadId, usize>,
}

impl SemaphoreDeadlockState {
    pub fn new() -> Self {
        Self {
            totals: Vec::new(),
            allocations: BTreeMap::new(),
            waiting: BTreeMap::new(),
        }
    }

    pub fn register(&mut self, total: usize) -> usize {
        self.totals.push(total);
        self.totals.len() - 1
    }

    fn available(&self) -> Vec<isize> {
        let mut available = self
            .totals
            .iter()
            .map(|&count| count as isize)
            .collect::<Vec<_>>();
        for held in self.allocations.values() {
            for (&sem_id, &count) in held {
                available[sem_id] -= count as isize;
            }
        }
        available
    }

    pub fn would_deadlock(&self, tid: ThreadId, sem_id: usize) -> bool {
        if sem_id >= self.totals.len() {
            return true;
        }
        let mut available = self.available();
        if available[sem_id] > 0 {
            return false;
        }
        let mut waiting = self.waiting.clone();
        waiting.insert(tid, sem_id);
        let mut threads = BTreeSet::new();
        threads.extend(self.allocations.keys().copied());
        threads.extend(waiting.keys().copied());
        threads.insert(tid);
        let mut finished = BTreeSet::new();
        loop {
            let mut progressed = false;
            for thread in threads.iter().copied() {
                if finished.contains(&thread) {
                    continue;
                }
                let need_satisfied = waiting
                    .get(&thread)
                    .map(|&want| available[want] > 0)
                    .unwrap_or(true);
                if need_satisfied {
                    if let Some(held) = self.allocations.get(&thread) {
                        for (&held_id, &count) in held {
                            available[held_id] += count as isize;
                        }
                    }
                    finished.insert(thread);
                    progressed = true;
                }
            }
            if !progressed {
                break;
            }
        }
        finished.len() != threads.len()
    }

    pub fn acquire(&mut self, tid: ThreadId, sem_id: usize) {
        self.waiting.remove(&tid);
        *self
            .allocations
            .entry(tid)
            .or_default()
            .entry(sem_id)
            .or_default() += 1;
    }

    pub fn block(&mut self, tid: ThreadId, sem_id: usize) {
        self.waiting.insert(tid, sem_id);
    }

    pub fn release(&mut self, tid: ThreadId, sem_id: usize) {
        if let Some(held) = self.allocations.get_mut(&tid) {
            if let Some(count) = held.get_mut(&sem_id) {
                *count -= 1;
                if *count == 0 {
                    held.remove(&sem_id);
                }
            }
            if held.is_empty() {
                self.allocations.remove(&tid);
            }
        }
    }
}

/// 互斥锁死锁检测状态。
pub struct MutexDeadlockState {
    owners: Vec<Option<ThreadId>>,
    waiting: BTreeMap<ThreadId, usize>,
}

impl MutexDeadlockState {
    pub fn new() -> Self {
        Self {
            owners: Vec::new(),
            waiting: BTreeMap::new(),
        }
    }

    pub fn register(&mut self) -> usize {
        self.owners.push(None);
        self.owners.len() - 1
    }

    pub fn would_deadlock(&self, tid: ThreadId, mutex_id: usize) -> bool {
        if mutex_id >= self.owners.len() {
            return true;
        }
        if self.owners[mutex_id].is_none() {
            return false;
        }
        let mut available = self
            .owners
            .iter()
            .map(|owner| if owner.is_none() { 1isize } else { 0isize })
            .collect::<Vec<_>>();
        let mut allocations: BTreeMap<ThreadId, BTreeMap<usize, usize>> = BTreeMap::new();
        for (id, owner) in self.owners.iter().enumerate() {
            if let Some(owner) = owner {
                allocations.entry(*owner).or_default().insert(id, 1);
            }
        }
        let mut waiting = self.waiting.clone();
        waiting.insert(tid, mutex_id);
        let mut threads = BTreeSet::new();
        threads.extend(allocations.keys().copied());
        threads.extend(waiting.keys().copied());
        threads.insert(tid);
        let mut finished = BTreeSet::new();
        loop {
            let mut progressed = false;
            for thread in threads.iter().copied() {
                if finished.contains(&thread) {
                    continue;
                }
                let need_satisfied = waiting
                    .get(&thread)
                    .map(|&want| available[want] > 0)
                    .unwrap_or(true);
                if need_satisfied {
                    if let Some(held) = allocations.get(&thread) {
                        for (&held_id, &count) in held {
                            available[held_id] += count as isize;
                        }
                    }
                    finished.insert(thread);
                    progressed = true;
                }
            }
            if !progressed {
                break;
            }
        }
        finished.len() != threads.len()
    }

    pub fn lock_success(&mut self, tid: ThreadId, mutex_id: usize) {
        self.waiting.remove(&tid);
        self.owners[mutex_id] = Some(tid);
    }

    pub fn block(&mut self, tid: ThreadId, mutex_id: usize) {
        self.waiting.insert(tid, mutex_id);
    }

    pub fn unlock(&mut self, mutex_id: usize, waking_tid: Option<ThreadId>) {
        if let Some(tid) = waking_tid {
            self.waiting.remove(&tid);
            self.owners[mutex_id] = Some(tid);
        } else {
            self.owners[mutex_id] = None;
        }
    }
}

/// 线程级调度实体状态。
///
/// 该状态与 `Thread` 绑定，而不是和 `Process` 绑定：
/// 阻塞、唤醒、交互延迟和就绪等待都发生在线程层。
pub struct SchedEntity {
    /// 创建时间戳。
    pub created_at_ns: u64,
    /// 最近一次进入 ready 队列的时间。
    pub ready_since_ns: Option<u64>,
    /// 第一次获得 CPU 的时间。
    pub first_run_at_ns: Option<u64>,
    /// 结束时间戳。
    pub finished_at_ns: Option<u64>,
    /// 累计运行时间。
    pub total_runtime_ns: u64,
    /// 累计 ready 等待时间。
    pub total_wait_ns: u64,
    /// 最近一次唤醒时间。
    pub last_wakeup_at_ns: Option<u64>,
    /// 交互延迟样本（wakeup -> next run）。
    pub interaction_latencies_ns: Vec<u64>,
    /// 饥饿事件次数。
    pub starvation_events: usize,
    /// 被调度次数。
    pub dispatch_count: usize,
    /// 运行片段次数。
    pub run_slices: usize,
    /// 上一次完整 CPU burst。
    pub last_burst_ns: u64,
    /// 预测的下一次 CPU burst（SJF 使用指数平均）。
    pub burst_estimate_ns: u64,
    /// 简化 CFS 的虚拟运行时间。
    pub vruntime_ns: u64,
    /// 简化 CFS 的权重。
    pub weight: u64,
    /// MLFQ 当前队列层级。
    pub queue_level: usize,
    /// 当前队列已使用的 tick 数。
    pub tick_budget_used: u32,
}

impl SchedEntity {
    /// 默认 burst 估计。
    pub const DEFAULT_BURST_NS: u64 = 300_000;
    /// CFS 默认权重。
    pub const DEFAULT_WEIGHT: u64 = 1024;

    /// 创建空调度实体。
    pub fn new() -> Self {
        Self {
            created_at_ns: 0,
            ready_since_ns: None,
            first_run_at_ns: None,
            finished_at_ns: None,
            total_runtime_ns: 0,
            total_wait_ns: 0,
            last_wakeup_at_ns: None,
            interaction_latencies_ns: Vec::new(),
            starvation_events: 0,
            dispatch_count: 0,
            run_slices: 0,
            last_burst_ns: 0,
            burst_estimate_ns: Self::DEFAULT_BURST_NS,
            vruntime_ns: 0,
            weight: Self::DEFAULT_WEIGHT,
            queue_level: 0,
            tick_budget_used: 0,
        }
    }

    /// 在线程被真正纳入实验时调用。
    pub fn on_created(&mut self, now_ns: u64) {
        self.created_at_ns = now_ns;
        self.ready_since_ns = Some(now_ns);
        self.first_run_at_ns = None;
        self.finished_at_ns = None;
        self.total_runtime_ns = 0;
        self.total_wait_ns = 0;
        self.last_wakeup_at_ns = None;
        self.interaction_latencies_ns.clear();
        self.starvation_events = 0;
        self.dispatch_count = 0;
        self.run_slices = 0;
        self.last_burst_ns = 0;
        self.burst_estimate_ns = Self::DEFAULT_BURST_NS;
        self.vruntime_ns = 0;
        self.weight = Self::DEFAULT_WEIGHT;
        self.queue_level = 0;
        self.tick_budget_used = 0;
    }

    /// 进入 ready 队列。
    pub fn on_ready(&mut self, now_ns: u64) {
        self.ready_since_ns = Some(now_ns);
    }

    /// 被唤醒并重新进入 ready 队列。
    pub fn on_wakeup(&mut self, now_ns: u64) {
        self.last_wakeup_at_ns = Some(now_ns);
        self.on_ready(now_ns);
    }

    /// 获得 CPU。
    pub fn on_dispatch(&mut self, now_ns: u64, starvation_threshold_ns: u64) {
        self.dispatch_count += 1;
        if self.first_run_at_ns.is_none() {
            self.first_run_at_ns = Some(now_ns);
        }
        if let Some(ready_since_ns) = self.ready_since_ns.take() {
            let wait_ns = now_ns.saturating_sub(ready_since_ns);
            self.total_wait_ns = self.total_wait_ns.saturating_add(wait_ns);
            if wait_ns >= starvation_threshold_ns {
                self.starvation_events += 1;
            }
            if let Some(wakeup_at_ns) = self.last_wakeup_at_ns.take() {
                self.interaction_latencies_ns
                    .push(now_ns.saturating_sub(wakeup_at_ns));
            }
        }
    }

    /// 完成一个运行片段。
    pub fn record_run(&mut self, burst_ns: u64) {
        self.run_slices += 1;
        self.total_runtime_ns = self.total_runtime_ns.saturating_add(burst_ns);
        self.last_burst_ns = burst_ns;
        self.burst_estimate_ns = (self.burst_estimate_ns.saturating_add(burst_ns.max(1))) / 2;
    }

    /// 线程退出。
    pub fn on_finish(&mut self, now_ns: u64) {
        self.finished_at_ns = Some(now_ns);
    }

    /// 周转时间。
    pub fn turnaround_ns(&self) -> u64 {
        self.finished_at_ns
            .unwrap_or(self.created_at_ns)
            .saturating_sub(self.created_at_ns)
    }
}

/// 线程（执行单元）
///
/// 每个线程有独立的 TID 和上下文（寄存器状态、satp）。
/// 同一进程的多个线程共享地址空间。
pub struct Thread {
    /// 线程 ID（不可变）
    pub tid: ThreadId,
    /// 执行上下文（包含 LocalContext + satp）
    pub context: ForeignContext,
    /// 线程级调度状态。
    pub sched: SchedEntity,
}

impl Thread {
    /// 创建新线程
    pub fn new(satp: usize, context: LocalContext) -> Self {
        Self {
            tid: ThreadId::new(),
            context: ForeignContext { context, satp },
            sched: SchedEntity::new(),
        }
    }
}

/// 进程（资源容器）
///
/// 管理地址空间、文件描述符、同步原语、信号等共享资源。
/// 一个进程可以包含多个线程。
pub struct Process {
    /// 进程 ID
    pub pid: ProcId,
    /// 地址空间（所有线程共享）
    pub address_space: AddressSpace<Sv39, Sv39Manager>,
    /// 文件描述符表（所有线程共享）
    pub fd_table: Vec<Option<Mutex<Fd>>>,
    /// 信号处理器
    pub signal: Box<dyn Signal>,
    /// 信号量列表（**本章新增**，所有线程共享）
    pub semaphore_list: Vec<Option<Arc<Semaphore>>>,
    /// 互斥锁列表（**本章新增**，所有线程共享）
    pub mutex_list: Vec<Option<Arc<dyn MutexTrait>>>,
    /// 条件变量列表（**本章新增**，所有线程共享）
    pub condvar_list: Vec<Option<Arc<Condvar>>>,
    /// 是否启用死锁检测。
    pub deadlock_detect_enabled: bool,
    /// 信号量死锁检测状态。
    pub semaphore_deadlock: SemaphoreDeadlockState,
    /// 互斥锁死锁检测状态。
    pub mutex_deadlock: MutexDeadlockState,
}

impl Process {
    /// exec：替换当前进程的地址空间和主线程上下文
    ///
    /// 注意：只支持单线程进程执行 exec
    pub fn exec(&mut self, elf: ElfFile, current_thread: &mut Thread) {
        let (proc, thread) = Process::from_elf(elf).unwrap();
        self.address_space = proc.address_space;
        current_thread.context = thread.context;
        current_thread.sched = SchedEntity::new();
    }

    /// fork：创建子进程（复制地址空间和主线程上下文）
    ///
    /// 子进程继承父进程的地址空间（深拷贝）、文件描述符和信号配置。
    /// 同步原语列表不继承（子进程创建空的列表）。
    pub fn fork(&mut self, current_thread: &Thread) -> Option<(Self, Thread)> {
        let pid = ProcId::new();
        // 深拷贝地址空间
        let parent_addr_space = &self.address_space;
        let mut address_space: AddressSpace<Sv39, Sv39Manager> = AddressSpace::new();
        parent_addr_space.cloneself(&mut address_space);
        map_portal(&address_space);
        // 复制主线程上下文
        let context = current_thread.context.context.clone();
        let satp = (8 << 60) | address_space.root_ppn().val();
        let thread = Thread::new(satp, context);
        // 复制文件描述符表
        let new_fd_table: Vec<Option<Mutex<Fd>>> = self
            .fd_table
            .iter()
            .map(|fd| fd.as_ref().map(|f| Mutex::new(f.lock().clone())))
            .collect();
        Some((
            Self {
                pid,
                address_space,
                fd_table: new_fd_table,
                signal: self.signal.from_fork(),
                // 子进程的同步原语列表初始为空
                semaphore_list: Vec::new(),
                mutex_list: Vec::new(),
                condvar_list: Vec::new(),
                deadlock_detect_enabled: false,
                semaphore_deadlock: SemaphoreDeadlockState::new(),
                mutex_deadlock: MutexDeadlockState::new(),
            },
            thread,
        ))
    }

    /// 从 ELF 文件创建进程和主线程
    ///
    /// 解析 ELF 段，建立地址空间，分配用户栈，创建初始上下文。
    pub fn from_elf(elf: ElfFile) -> Option<(Self, Thread)> {
        let entry = match elf.header.pt2 {
            HeaderPt2::Header64(pt2)
                if pt2.type_.as_type() == header::Type::Executable
                    && pt2.machine.as_machine() == Machine::RISC_V =>
            {
                pt2.entry_point as usize
            }
            _ => None?,
        };

        const PAGE_SIZE: usize = 1 << Sv39::PAGE_BITS;
        const PAGE_MASK: usize = PAGE_SIZE - 1;

        let mut address_space = AddressSpace::new();
        for program in elf.program_iter() {
            if !matches!(program.get_type(), Ok(program::Type::Load)) {
                continue;
            }
            let off_file = program.offset() as usize;
            let len_file = program.file_size() as usize;
            let off_mem = program.virtual_addr() as usize;
            let end_mem = off_mem + program.mem_size() as usize;
            assert_eq!(off_file & PAGE_MASK, off_mem & PAGE_MASK);
            let mut flags: [u8; 5] = *b"U___V";
            if program.flags().is_execute() {
                flags[1] = b'X';
            }
            if program.flags().is_write() {
                flags[2] = b'W';
            }
            if program.flags().is_read() {
                flags[3] = b'R';
            }
            address_space.map(
                VAddr::new(off_mem).floor()..VAddr::new(end_mem).ceil(),
                &elf.input[off_file..][..len_file],
                off_mem & PAGE_MASK,
                parse_flags(unsafe { core::str::from_utf8_unchecked(&flags) }).unwrap(),
            );
        }
        // 分配 2 页用户栈
        let stack = unsafe {
            alloc_zeroed(Layout::from_size_align_unchecked(
                2 << Sv39::PAGE_BITS,
                1 << Sv39::PAGE_BITS,
            ))
        };
        address_space.map_extern(
            VPN::new((1 << 26) - 2)..VPN::new(1 << 26),
            PPN::new(stack as usize >> Sv39::PAGE_BITS),
            build_flags("U_WRV"),
        );
        map_portal(&address_space);
        let satp = (8 << 60) | address_space.root_ppn().val();
        let mut context = LocalContext::user(entry);
        *context.sp_mut() = 1 << 38;
        let thread = Thread::new(satp, context);

        Some((
            Self {
                pid: ProcId::new(),
                address_space,
                fd_table: vec![
                    // stdin
                    Some(Mutex::new(Fd::Empty {
                        read: true,
                        write: false,
                    })),
                    // stdout
                    Some(Mutex::new(Fd::Empty {
                        read: false,
                        write: true,
                    })),
                    // stderr
                    Some(Mutex::new(Fd::Empty {
                        read: false,
                        write: true,
                    })),
                ],
                signal: Box::new(SignalImpl::new()),
                semaphore_list: Vec::new(),
                mutex_list: Vec::new(),
                condvar_list: Vec::new(),
                deadlock_detect_enabled: false,
                semaphore_deadlock: SemaphoreDeadlockState::new(),
                mutex_deadlock: MutexDeadlockState::new(),
            },
            thread,
        ))
    }
}
