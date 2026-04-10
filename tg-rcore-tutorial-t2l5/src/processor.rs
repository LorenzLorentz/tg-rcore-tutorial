//! 处理器与调度模块
//!
//! ## 与第七章的区别
//!
//! 第七章使用 `PManager`（进程管理器）作为全局处理器类型；
//! 第八章使用 `PThreadManager`（进程 + 线程双层管理器），
//! 支持一个进程拥有多个线程。
//!
//! ## 核心类型
//!
//! - `ProcessorInner = PThreadManager<Process, Thread, ThreadManager, ProcManager>`
//! - `ThreadManager`：管理线程实体和就绪队列
//! - `ProcManager`：管理进程实体
//!
//! 教程阅读建议：
//!
//! - 先看 `ProcessorInner` 类型别名：先建立“统一入口，双层实体”的心智模型；
//! - 再看 `ThreadManager` 与 `ProcManager` 的 `Manage` 实现：理解两层对象如何独立维护；
//! - 最后看 `Schedule<ThreadId>`：明确调度粒度已经从进程切换为线程。

use crate::process::{Process, Thread};
use alloc::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    vec::Vec,
};
use core::{
    cell::UnsafeCell,
    sync::atomic::{AtomicUsize, Ordering},
};
use spin::Mutex as SpinMutex;
use tg_task_manage::{Manage, PThreadManager, ProcId, Schedule, ThreadId};

/// 处理器内部类型（双层管理器）
pub type ProcessorInner = PThreadManager<Process, Thread, ThreadManager, ProcManager>;

/// 全局处理器包装（通过 `UnsafeCell` 允许内部可变）
pub struct Processor {
    inner: UnsafeCell<ProcessorInner>,
}

unsafe impl Sync for Processor {}

impl Processor {
    /// 创建新处理器
    pub const fn new() -> Self {
        Self {
            inner: UnsafeCell::new(PThreadManager::new()),
        }
    }

    /// 获取内部可变引用
    #[inline]
    pub fn get_mut(&self) -> &mut ProcessorInner {
        unsafe { &mut (*self.inner.get()) }
    }
}

/// 全局处理器实例
pub static PROCESSOR: Processor = Processor::new();

#[derive(Clone, Copy)]
pub enum KernelBugClass {
    Exact,
    Heuristic,
}

/// 内核同步实验计数器。
pub struct KernelMetrics {
    /// 总上下文切换次数。
    pub context_switches: AtomicUsize,
    /// 因同步原语阻塞的次数。
    pub blocked_sync_ops: AtomicUsize,
    /// 同步原语唤醒次数。
    pub wakeups: AtomicUsize,
    /// bug 总次数。
    pub bug_total: AtomicUsize,
    /// 精确型 bug 次数。
    pub bug_exact: AtomicUsize,
    /// 启发式 bug 次数。
    pub bug_heuristic: AtomicUsize,
    /// 统计型 bug 次数。
    pub bug_statistical: AtomicUsize,
}

impl KernelMetrics {
    /// 创建空计数器。
    pub const fn new() -> Self {
        Self {
            context_switches: AtomicUsize::new(0),
            blocked_sync_ops: AtomicUsize::new(0),
            wakeups: AtomicUsize::new(0),
            bug_total: AtomicUsize::new(0),
            bug_exact: AtomicUsize::new(0),
            bug_heuristic: AtomicUsize::new(0),
            bug_statistical: AtomicUsize::new(0),
        }
    }

    pub fn record_bug(&self, class: KernelBugClass) {
        self.bug_total.fetch_add(1, Ordering::Relaxed);
        match class {
            KernelBugClass::Exact => {
                self.bug_exact.fetch_add(1, Ordering::Relaxed);
            }
            KernelBugClass::Heuristic => {
                self.bug_heuristic.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// 清零计数器。
    pub fn reset(&self) {
        self.context_switches.store(0, Ordering::Relaxed);
        self.blocked_sync_ops.store(0, Ordering::Relaxed);
        self.wakeups.store(0, Ordering::Relaxed);
        self.bug_total.store(0, Ordering::Relaxed);
        self.bug_exact.store(0, Ordering::Relaxed);
        self.bug_heuristic.store(0, Ordering::Relaxed);
        self.bug_statistical.store(0, Ordering::Relaxed);
    }
}

/// 全局内核同步实验计数器。
pub static KERNEL_METRICS: KernelMetrics = KernelMetrics::new();

pub struct ActiveEntities {
    procs: SpinMutex<BTreeSet<ProcId>>,
    threads: SpinMutex<BTreeSet<ThreadId>>,
}

impl ActiveEntities {
    pub const fn new() -> Self {
        Self {
            procs: SpinMutex::new(BTreeSet::new()),
            threads: SpinMutex::new(BTreeSet::new()),
        }
    }

    pub fn add_proc(&self, pid: ProcId) {
        self.procs.lock().insert(pid);
    }

    pub fn remove_proc(&self, pid: ProcId) {
        self.procs.lock().remove(&pid);
    }

    pub fn add_thread(&self, tid: ThreadId) {
        self.threads.lock().insert(tid);
    }

    pub fn remove_thread(&self, tid: ThreadId) {
        self.threads.lock().remove(&tid);
    }

    pub fn active_proc_ids(&self) -> Vec<ProcId> {
        self.procs.lock().iter().copied().collect()
    }

    pub fn active_thread_count(&self) -> usize {
        self.threads.lock().len()
    }
}

pub static ACTIVE_ENTITIES: ActiveEntities = ActiveEntities::new();

/// 线程管理器
///
/// 维护所有线程实体和就绪队列。
/// 使用 FIFO 调度策略。
pub struct ThreadManager {
    /// 线程实体表（TID → Thread）
    tasks: BTreeMap<ThreadId, Thread>,
    /// 就绪队列
    ready_queue: VecDeque<ThreadId>,
}

impl ThreadManager {
    /// 创建空的线程管理器
    pub fn new() -> Self {
        Self {
            tasks: BTreeMap::new(),
            ready_queue: VecDeque::new(),
        }
    }
}

impl Manage<Thread, ThreadId> for ThreadManager {
    /// 插入线程实体
    #[inline]
    fn insert(&mut self, id: ThreadId, task: Thread) {
        self.tasks.insert(id, task);
        ACTIVE_ENTITIES.add_thread(id);
    }
    /// 获取线程可变引用
    #[inline]
    fn get_mut(&mut self, id: ThreadId) -> Option<&mut Thread> {
        self.tasks.get_mut(&id)
    }
    /// 删除线程实体
    #[inline]
    fn delete(&mut self, id: ThreadId) {
        self.tasks.remove(&id);
        ACTIVE_ENTITIES.remove_thread(id);
    }
}

impl Schedule<ThreadId> for ThreadManager {
    /// 加入就绪队列
    fn add(&mut self, id: ThreadId) {
        self.ready_queue.push_back(id);
    }
    /// 取出下一个就绪线程
    fn fetch(&mut self) -> Option<ThreadId> {
        let next = self.ready_queue.pop_front();
        if next.is_some() {
            KERNEL_METRICS
                .context_switches
                .fetch_add(1, Ordering::Relaxed);
        }
        next
    }
}

/// 进程管理器
///
/// 维护所有进程实体（PID → Process）。
pub struct ProcManager {
    procs: BTreeMap<ProcId, Process>,
}

impl ProcManager {
    /// 创建空的进程管理器
    pub fn new() -> Self {
        Self {
            procs: BTreeMap::new(),
        }
    }
}

impl Manage<Process, ProcId> for ProcManager {
    /// 插入进程实体
    #[inline]
    fn insert(&mut self, id: ProcId, item: Process) {
        self.procs.insert(id, item);
        ACTIVE_ENTITIES.add_proc(id);
    }
    /// 获取进程可变引用
    #[inline]
    fn get_mut(&mut self, id: ProcId) -> Option<&mut Process> {
        self.procs.get_mut(&id)
    }
    /// 删除进程实体
    #[inline]
    fn delete(&mut self, id: ProcId) {
        self.procs.remove(&id);
        ACTIVE_ENTITIES.remove_proc(id);
    }
}
