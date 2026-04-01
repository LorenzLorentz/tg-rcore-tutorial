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
    /// 是否启用死锁检测。
    pub deadlock_detect_enabled: bool,
    /// 信号量死锁检测状态。
    pub semaphore_deadlock: SemaphoreDeadlockState,
    /// 互斥锁死锁检测状态。
    pub mutex_deadlock: MutexDeadlockState,
    /// 堆底地址。
    pub heap_bottom: usize,
    /// 当前程序 break 位置。
    pub program_brk: usize,
}

impl Process {
    /// exec：替换当前进程的地址空间和主线程上下文
    ///
    /// 注意：只支持单线程进程执行 exec
    pub fn exec(&mut self, elf: ElfFile) {
        let (proc, thread) = Process::from_elf(elf).unwrap();
        self.address_space = proc.address_space;
        self.heap_bottom = proc.heap_bottom;
        self.program_brk = proc.program_brk;
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
                deadlock_detect_enabled: false,
                semaphore_deadlock: SemaphoreDeadlockState::new(),
                mutex_deadlock: MutexDeadlockState::new(),
                heap_bottom: self.heap_bottom,
                program_brk: self.program_brk,
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
        let mut max_end_va = 0usize;
        for program in elf.program_iter() {
            if !matches!(program.get_type(), Ok(program::Type::Load)) {
                continue;
            }
            let off_file = program.offset() as usize;
            let len_file = program.file_size() as usize;
            let off_mem = program.virtual_addr() as usize;
            let end_mem = off_mem + program.mem_size() as usize;
            assert_eq!(off_file & PAGE_MASK, off_mem & PAGE_MASK);
            if end_mem > max_end_va {
                max_end_va = end_mem;
            }
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
        let heap_bottom = VAddr::<Sv39>::new(max_end_va).ceil().base().val();
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
                heap_bottom,
                program_brk: heap_bottom,
            },
            thread,
        ))
    }

    /// 修改程序 break 位置（实现 sbrk）。
    pub fn change_program_brk(&mut self, size: isize) -> Option<usize> {
        let old_brk = self.program_brk;
        let new_brk = self.program_brk as isize + size;
        if new_brk < self.heap_bottom as isize {
            return None;
        }
        let new_brk = new_brk as usize;

        let old_brk_ceil = VAddr::<Sv39>::new(old_brk).ceil();
        let new_brk_ceil = VAddr::<Sv39>::new(new_brk).ceil();

        if size > 0 {
            if new_brk_ceil.val() > old_brk_ceil.val() {
                self.address_space
                    .map(old_brk_ceil..new_brk_ceil, &[], 0, build_flags("U_WRV"));
            }
        } else if size < 0 && old_brk_ceil.val() > new_brk_ceil.val() {
            self.address_space.unmap(new_brk_ceil..old_brk_ceil);
        }

        self.program_brk = new_brk;
        Some(old_brk)
    }
}
