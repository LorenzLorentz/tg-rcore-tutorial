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

use crate::{
    PROCESSOR, Sv39, Sv39Manager, build_flags, fs::Fd, map_portal, parse_flags,
    processor::ProcessorInner,
};
use alloc::{
    alloc::alloc_zeroed,
    boxed::Box,
    collections::{BTreeMap, BTreeSet},
    format,
    string::String,
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

    pub fn waiting_sem(&self, tid: ThreadId) -> Option<usize> {
        self.waiting.get(&tid).copied()
    }

    pub fn held_resources_by(&self, tid: ThreadId) -> Vec<(usize, usize)> {
        self.allocations
            .get(&tid)
            .map(|held| held.iter().map(|(&sem_id, &count)| (sem_id, count)).collect())
            .unwrap_or_default()
    }

    pub fn waiting_entries(&self) -> Vec<(ThreadId, usize)> {
        self.waiting
            .iter()
            .map(|(&tid, &sem_id)| (tid, sem_id))
            .collect()
    }

    pub fn clear_waiting(&mut self, tid: ThreadId) {
        self.waiting.remove(&tid);
    }

    pub fn describe_deadlock_snapshot(&self, tid: ThreadId, sem_id: usize) -> String {
        let available = self
            .available()
            .iter()
            .enumerate()
            .map(|(id, count)| format!("S{id}:{count}"))
            .collect::<Vec<_>>()
            .join(",");
        let waiting = self
            .waiting
            .iter()
            .map(|(&waiting_tid, &waiting_sem)| {
                format!("T{}->S{}", waiting_tid.get_usize(), waiting_sem)
            })
            .collect::<Vec<_>>()
            .join(",");
        let allocations = self
            .allocations
            .iter()
            .map(|(&holder_tid, held)| {
                let held = held
                    .iter()
                    .map(|(&held_sem, &count)| format!("S{held_sem}:{count}"))
                    .collect::<Vec<_>>()
                    .join(",");
                format!("T{}=[{}]", holder_tid.get_usize(), held)
            })
            .collect::<Vec<_>>()
            .join(";");
        format!(
            "request=T{}->S{} available=[{}] waiting=[{}] allocations=[{}]",
            tid.get_usize(),
            sem_id,
            available,
            waiting,
            allocations
        )
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

    pub fn owner_of(&self, mutex_id: usize) -> Option<ThreadId> {
        self.owners.get(mutex_id).copied().flatten()
    }

    pub fn held_mutexes_by(&self, tid: ThreadId) -> Vec<usize> {
        self.owners
            .iter()
            .enumerate()
            .filter_map(|(mutex_id, owner)| owner.filter(|owner| *owner == tid).map(|_| mutex_id))
            .collect()
    }

    pub fn waiting_mutex(&self, tid: ThreadId) -> Option<usize> {
        self.waiting.get(&tid).copied()
    }

    pub fn waiting_entries(&self) -> Vec<(ThreadId, usize)> {
        self.waiting
            .iter()
            .map(|(&tid, &mutex_id)| (tid, mutex_id))
            .collect()
    }

    pub fn clear_waiting(&mut self, tid: ThreadId) {
        self.waiting.remove(&tid);
    }

    pub fn describe_wait_chain(&self, start_tid: ThreadId, start_mutex_id: usize) -> String {
        let mut parts = vec![
            format!("T{}", start_tid.get_usize()),
            format!("M{start_mutex_id}"),
        ];
        let mut current_tid = start_tid;
        let mut current_mutex = start_mutex_id;
        let mut seen = BTreeSet::new();
        seen.insert((current_tid, current_mutex));
        while let Some(owner) = self.owner_of(current_mutex) {
            parts.push(format!("T{}", owner.get_usize()));
            if owner == start_tid {
                break;
            }
            if let Some(next_mutex) = self.waiting.get(&owner).copied() {
                parts.push(format!("M{next_mutex}"));
                if !seen.insert((owner, next_mutex)) {
                    break;
                }
                current_tid = owner;
                current_mutex = next_mutex;
            } else {
                let _ = current_tid;
                break;
            }
        }
        parts.join(" -> ")
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

#[derive(Clone, Copy)]
pub struct CondvarWaitState {
    pub condvar_id: usize,
    pub mutex_id: usize,
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
}

impl Thread {
    /// 创建新线程
    pub fn new(satp: usize, context: LocalContext) -> Self {
        Self {
            tid: ThreadId::new(),
            context: ForeignContext { context, satp },
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
    /// 阻塞在 condvar 上的线程。
    pub condvar_waiting: BTreeMap<ThreadId, CondvarWaitState>,
    /// 是否启用死锁检测。
    pub deadlock_detect_enabled: bool,
    /// 信号量死锁检测状态。
    pub semaphore_deadlock: SemaphoreDeadlockState,
    /// 互斥锁死锁检测状态。
    pub mutex_deadlock: MutexDeadlockState,
}

impl Process {
    pub fn record_condvar_wait(&mut self, tid: ThreadId, condvar_id: usize, mutex_id: usize) {
        self.condvar_waiting.insert(
            tid,
            CondvarWaitState {
                condvar_id,
                mutex_id,
            },
        );
    }

    pub fn clear_condvar_wait(&mut self, tid: ThreadId) {
        self.condvar_waiting.remove(&tid);
    }

    pub fn describe_blocked_threads(&self) -> String {
        let mut lines = Vec::new();
        for (tid, mutex_id) in self.mutex_deadlock.waiting_entries() {
            let owner = self
                .mutex_deadlock
                .owner_of(mutex_id)
                .map(|owner| format!("T{}", owner.get_usize()))
                .unwrap_or_else(|| "none".into());
            lines.push(format!(
                "T{} waiting mutex M{} owner={}",
                tid.get_usize(),
                mutex_id,
                owner
            ));
        }
        for (tid, sem_id) in self.semaphore_deadlock.waiting_entries() {
            let holders = self
                .semaphore_deadlock
                .allocations
                .iter()
                .filter_map(|(&holder_tid, held)| {
                    held.get(&sem_id)
                        .copied()
                        .map(|count| format!("T{}:{}", holder_tid.get_usize(), count))
                })
                .collect::<Vec<_>>()
                .join(",");
            lines.push(format!(
                "T{} waiting semaphore S{} holders=[{}]",
                tid.get_usize(),
                sem_id,
                holders
            ));
        }
        for (&tid, wait_state) in &self.condvar_waiting {
            lines.push(format!(
                "T{} waiting condvar C{} via mutex M{}",
                tid.get_usize(),
                wait_state.condvar_id,
                wait_state.mutex_id
            ));
        }
        if lines.is_empty() {
            "no_blocked_threads".into()
        } else {
            lines.join("; ")
        }
    }

    pub fn describe_thread_sync_state(&self, tid: ThreadId) -> Option<String> {
        let held_mutexes = self.mutex_deadlock.held_mutexes_by(tid);
        let held_semaphores = self.semaphore_deadlock.held_resources_by(tid);
        let waiting_mutex = self.mutex_deadlock.waiting_mutex(tid);
        let waiting_sem = self.semaphore_deadlock.waiting_sem(tid);
        let waiting_condvar = self.condvar_waiting.get(&tid).copied();
        if held_mutexes.is_empty()
            && held_semaphores.is_empty()
            && waiting_mutex.is_none()
            && waiting_sem.is_none()
            && waiting_condvar.is_none()
        {
            return None;
        }
        let mut parts = Vec::new();
        if !held_mutexes.is_empty() {
            parts.push(format!(
                "held_mutexes=[{}]",
                held_mutexes
                    .iter()
                    .map(|mutex_id| format!("M{mutex_id}"))
                    .collect::<Vec<_>>()
                    .join(",")
            ));
        }
        if !held_semaphores.is_empty() {
            parts.push(format!(
                "held_semaphores=[{}]",
                held_semaphores
                    .iter()
                    .map(|(sem_id, count)| format!("S{sem_id}:{count}"))
                    .collect::<Vec<_>>()
                    .join(",")
            ));
        }
        if let Some(mutex_id) = waiting_mutex {
            parts.push(format!("waiting_mutex=M{mutex_id}"));
        }
        if let Some(sem_id) = waiting_sem {
            parts.push(format!("waiting_semaphore=S{sem_id}"));
        }
        if let Some(wait_state) = waiting_condvar {
            parts.push(format!(
                "waiting_condvar=C{} via M{}",
                wait_state.condvar_id, wait_state.mutex_id
            ));
        }
        Some(parts.join(" "))
    }

    /// exec：替换当前进程的地址空间和主线程上下文
    ///
    /// 注意：只支持单线程进程执行 exec
    pub fn exec(&mut self, elf: ElfFile) {
        let (proc, thread) = Process::from_elf(elf).unwrap();
        self.address_space = proc.address_space;
        let processor: *mut ProcessorInner = PROCESSOR.get_mut() as *mut ProcessorInner;
        unsafe {
            let pthreads = (*processor).get_thread(self.pid).unwrap();
            (*processor).get_task(pthreads[0]).unwrap().context = thread.context;
        }
    }

    /// fork：创建子进程（复制地址空间和主线程上下文）
    ///
    /// 子进程继承父进程的地址空间（深拷贝）、文件描述符和信号配置。
    /// 同步原语列表不继承（子进程创建空的列表）。
    pub fn fork(&mut self) -> Option<(Self, Thread)> {
        let pid = ProcId::new();
        // 深拷贝地址空间
        let parent_addr_space = &self.address_space;
        let mut address_space: AddressSpace<Sv39, Sv39Manager> = AddressSpace::new();
        parent_addr_space.cloneself(&mut address_space);
        map_portal(&address_space);
        // 复制主线程上下文
        let processor: *mut ProcessorInner = PROCESSOR.get_mut() as *mut ProcessorInner;
        let pthreads = unsafe { (*processor).get_thread(self.pid).unwrap() };
        let context = unsafe {
            (*processor)
                .get_task(pthreads[0])
                .unwrap()
                .context
                .context
                .clone()
        };
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
                condvar_waiting: BTreeMap::new(),
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
                condvar_waiting: BTreeMap::new(),
                deadlock_detect_enabled: false,
                semaphore_deadlock: SemaphoreDeadlockState::new(),
                mutex_deadlock: MutexDeadlockState::new(),
            },
            thread,
        ))
    }
}
