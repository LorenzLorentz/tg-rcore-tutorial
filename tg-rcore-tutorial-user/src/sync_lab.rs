use crate::{
    ClockId, TimeSpec, clock_gettime, condvar_wait, mutex_lock, semaphore_create, semaphore_down,
    semaphore_up, trace,
};
use core::sync::atomic::{AtomicUsize, Ordering};

/// t2l5 trace 请求：清零内核计数器。
pub const T2L5_TRACE_RESET_METRICS: usize = 0x200;
/// t2l5 trace 请求：读取上下文切换次数。
pub const T2L5_TRACE_GET_CONTEXT_SWITCHES: usize = 0x201;
/// t2l5 trace 请求：读取同步阻塞次数。
pub const T2L5_TRACE_GET_BLOCKED_SYNC: usize = 0x202;
/// t2l5 trace 请求：读取同步唤醒次数。
pub const T2L5_TRACE_GET_WAKEUPS: usize = 0x203;
/// t2l5 trace 请求：读取 bug 总数。
pub const T2L5_TRACE_GET_BUG_TOTAL: usize = 0x208;
/// t2l5 trace 请求：读取精确型 bug 数。
pub const T2L5_TRACE_GET_BUG_EXACT: usize = 0x209;
/// t2l5 trace 请求：读取启发式 bug 数。
pub const T2L5_TRACE_GET_BUG_HEURISTIC: usize = 0x20a;
/// t2l5 trace 请求：读取统计型 bug 数。
pub const T2L5_TRACE_GET_BUG_STATISTICAL: usize = 0x20b;
/// t4l45 trace 请求：读取当前 hart 编号。
pub const T4L45_TRACE_GET_CURRENT_HART: usize = 0x204;

/// 内核计数器快照。
#[derive(Clone, Copy)]
pub struct KernelMetricSnapshot {
    /// 总上下文切换次数。
    pub context_switches: usize,
    /// 因同步原语阻塞的次数。
    pub blocked_sync_ops: usize,
    /// 同步原语唤醒次数。
    pub wakeups: usize,
    /// bug 总数。
    pub bug_total: usize,
    /// 精确型 bug 数。
    pub bug_exact: usize,
    /// 启发式 bug 数。
    pub bug_heuristic: usize,
    /// 统计型 bug 数。
    pub bug_statistical: usize,
}

impl KernelMetricSnapshot {
    /// 与更早快照求差。
    pub fn diff(self, earlier: Self) -> Self {
        Self {
            context_switches: self.context_switches - earlier.context_switches,
            blocked_sync_ops: self.blocked_sync_ops - earlier.blocked_sync_ops,
            wakeups: self.wakeups - earlier.wakeups,
            bug_total: self.bug_total - earlier.bug_total,
            bug_exact: self.bug_exact - earlier.bug_exact,
            bug_heuristic: self.bug_heuristic - earlier.bug_heuristic,
            bug_statistical: self.bug_statistical - earlier.bug_statistical,
        }
    }
}

/// 共享统计器。
pub struct AtomicStats {
    attempts: AtomicUsize,
    contentions: AtomicUsize,
    acquisitions: AtomicUsize,
    total_wait_us: AtomicUsize,
    max_wait_us: AtomicUsize,
    total_hold_us: AtomicUsize,
    max_hold_us: AtomicUsize,
    starvation: AtomicUsize,
}

/// 统计快照。
#[derive(Clone, Copy)]
pub struct StatsSnapshot {
    /// 尝试次数。
    pub attempts: usize,
    /// 发生竞争的次数。
    pub contentions: usize,
    /// 成功获取次数。
    pub acquisitions: usize,
    /// 累计等待时间。
    pub total_wait_us: usize,
    /// 最大等待时间。
    pub max_wait_us: usize,
    /// 累计持有时间。
    pub total_hold_us: usize,
    /// 最大持有时间。
    pub max_hold_us: usize,
    /// 超过阈值的等待次数。
    pub starvation: usize,
}

impl StatsSnapshot {
    /// 平均等待时间。
    pub fn avg_wait_us(&self) -> usize {
        if self.acquisitions == 0 {
            0
        } else {
            self.total_wait_us / self.acquisitions
        }
    }

    /// 平均持有时间。
    pub fn avg_hold_us(&self) -> usize {
        if self.acquisitions == 0 {
            0
        } else {
            self.total_hold_us / self.acquisitions
        }
    }
}

impl AtomicStats {
    /// 创建空统计器。
    pub const fn new() -> Self {
        Self {
            attempts: AtomicUsize::new(0),
            contentions: AtomicUsize::new(0),
            acquisitions: AtomicUsize::new(0),
            total_wait_us: AtomicUsize::new(0),
            max_wait_us: AtomicUsize::new(0),
            total_hold_us: AtomicUsize::new(0),
            max_hold_us: AtomicUsize::new(0),
            starvation: AtomicUsize::new(0),
        }
    }

    /// 记录一次获取成功。
    pub fn record_wait(&self, wait_us: usize, contended: bool, starvation_threshold_us: usize) {
        self.attempts.fetch_add(1, Ordering::Relaxed);
        self.acquisitions.fetch_add(1, Ordering::Relaxed);
        if contended {
            self.contentions.fetch_add(1, Ordering::Relaxed);
        }
        self.total_wait_us.fetch_add(wait_us, Ordering::Relaxed);
        update_max(&self.max_wait_us, wait_us);
        if starvation_threshold_us != 0 && wait_us > starvation_threshold_us {
            self.starvation.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// 记录一次持有时间。
    pub fn record_hold(&self, hold_us: usize) {
        self.total_hold_us.fetch_add(hold_us, Ordering::Relaxed);
        update_max(&self.max_hold_us, hold_us);
    }

    /// 记录一次只关心持有时长的样本。
    pub fn record_hold_sample(&self, hold_us: usize) {
        self.attempts.fetch_add(1, Ordering::Relaxed);
        self.acquisitions.fetch_add(1, Ordering::Relaxed);
        self.record_hold(hold_us);
    }

    /// 读取快照。
    pub fn snapshot(&self) -> StatsSnapshot {
        StatsSnapshot {
            attempts: self.attempts.load(Ordering::Relaxed),
            contentions: self.contentions.load(Ordering::Relaxed),
            acquisitions: self.acquisitions.load(Ordering::Relaxed),
            total_wait_us: self.total_wait_us.load(Ordering::Relaxed),
            max_wait_us: self.max_wait_us.load(Ordering::Relaxed),
            total_hold_us: self.total_hold_us.load(Ordering::Relaxed),
            max_hold_us: self.max_hold_us.load(Ordering::Relaxed),
            starvation: self.starvation.load(Ordering::Relaxed),
        }
    }
}

/// 公平 ticket spinlock。
pub struct TicketSpinLock {
    next_ticket: AtomicUsize,
    serving: AtomicUsize,
}

/// 公平读写锁。
pub struct FairRwLock {
    service_queue: usize,
    resource: usize,
    read_mutex: usize,
    reader_count: AtomicUsize,
}

/// 读者优先读写锁。
pub struct ReaderPreferRwLock {
    resource: usize,
    read_mutex: usize,
    reader_count: AtomicUsize,
}

impl TicketSpinLock {
    /// 创建新的 ticket spinlock。
    pub const fn new() -> Self {
        Self {
            next_ticket: AtomicUsize::new(0),
            serving: AtomicUsize::new(0),
        }
    }

    /// 获取锁；返回值表示这次获取是否经历了竞争。
    pub fn lock(&self) -> bool {
        let ticket = self.next_ticket.fetch_add(1, Ordering::Relaxed);
        let mut contended = false;
        while self.serving.load(Ordering::Acquire) != ticket {
            contended = true;
            crate::sched_yield();
        }
        contended
    }

    /// 释放锁。
    pub fn unlock(&self) {
        self.serving.fetch_add(1, Ordering::Release);
    }
}

impl FairRwLock {
    /// 创建公平读写锁。
    pub fn new() -> Self {
        Self {
            service_queue: semaphore_create(1) as usize,
            resource: semaphore_create(1) as usize,
            read_mutex: semaphore_create(1) as usize,
            reader_count: AtomicUsize::new(0),
        }
    }

    /// 读加锁。
    pub fn read_lock(&self) -> bool {
        let mut contended = semaphore_down_contended(self.service_queue);
        contended |= semaphore_down_contended(self.read_mutex);
        let readers = self.reader_count.fetch_add(1, Ordering::SeqCst);
        if readers == 0 {
            contended |= semaphore_down_contended(self.resource);
        }
        semaphore_up(self.read_mutex);
        semaphore_up(self.service_queue);
        contended
    }

    /// 读解锁。
    pub fn read_unlock(&self) {
        let _ = semaphore_down_contended(self.read_mutex);
        let remaining = self.reader_count.fetch_sub(1, Ordering::SeqCst) - 1;
        if remaining == 0 {
            semaphore_up(self.resource);
        }
        semaphore_up(self.read_mutex);
    }

    /// 写加锁。
    pub fn write_lock(&self) -> bool {
        let mut contended = semaphore_down_contended(self.service_queue);
        contended |= semaphore_down_contended(self.resource);
        semaphore_up(self.service_queue);
        contended
    }

    /// 写解锁。
    pub fn write_unlock(&self) {
        semaphore_up(self.resource);
    }
}

impl ReaderPreferRwLock {
    /// 创建读者优先读写锁。
    pub fn new() -> Self {
        Self {
            resource: semaphore_create(1) as usize,
            read_mutex: semaphore_create(1) as usize,
            reader_count: AtomicUsize::new(0),
        }
    }

    /// 读加锁。
    pub fn read_lock(&self) -> bool {
        let mut contended = semaphore_down_contended(self.read_mutex);
        let readers = self.reader_count.fetch_add(1, Ordering::SeqCst);
        if readers == 0 {
            contended |= semaphore_down_contended(self.resource);
        }
        semaphore_up(self.read_mutex);
        contended
    }

    /// 读解锁。
    pub fn read_unlock(&self) {
        let _ = semaphore_down_contended(self.read_mutex);
        let remaining = self.reader_count.fetch_sub(1, Ordering::SeqCst) - 1;
        if remaining == 0 {
            semaphore_up(self.resource);
        }
        semaphore_up(self.read_mutex);
    }

    /// 写加锁。
    pub fn write_lock(&self) -> bool {
        semaphore_down_contended(self.resource)
    }

    /// 写解锁。
    pub fn write_unlock(&self) {
        semaphore_up(self.resource);
    }
}

/// 读取当前时间（微秒）。
pub fn now_us() -> usize {
    let mut time = TimeSpec::ZERO;
    clock_gettime(ClockId::CLOCK_MONOTONIC, &mut time as *mut _ as _);
    time.tv_sec * 1_000_000 + time.tv_nsec / 1_000
}

/// 清零内核计数器。
pub fn reset_kernel_metrics() {
    let _ = trace(T2L5_TRACE_RESET_METRICS, 0, 0);
}

/// 读取内核计数器。
pub fn kernel_metrics() -> KernelMetricSnapshot {
    KernelMetricSnapshot {
        context_switches: trace(T2L5_TRACE_GET_CONTEXT_SWITCHES, 0, 0) as usize,
        blocked_sync_ops: trace(T2L5_TRACE_GET_BLOCKED_SYNC, 0, 0) as usize,
        wakeups: trace(T2L5_TRACE_GET_WAKEUPS, 0, 0) as usize,
        bug_total: trace(T2L5_TRACE_GET_BUG_TOTAL, 0, 0) as usize,
        bug_exact: trace(T2L5_TRACE_GET_BUG_EXACT, 0, 0) as usize,
        bug_heuristic: trace(T2L5_TRACE_GET_BUG_HEURISTIC, 0, 0) as usize,
        bug_statistical: trace(T2L5_TRACE_GET_BUG_STATISTICAL, 0, 0) as usize,
    }
}

pub fn print_user_bug(class: &str, kind: &str, primitive: &str, details: &str) {
    crate::println!(
        "[t2l5-bug] source=user class={} kind={} primitive={} {}",
        class, kind, primitive, details
    );
}

/// 读取当前线程所在的 hart 编号。
pub fn current_hart_id() -> usize {
    trace(T4L45_TRACE_GET_CURRENT_HART, 0, 0) as usize
}

/// 包装 `mutex_lock`，返回是否发生过阻塞。
pub fn mutex_lock_contended(mutex_id: usize) -> bool {
    match mutex_lock(mutex_id) {
        0 => false,
        -1 => true,
        ret => panic!("unexpected mutex_lock return value: {ret}"),
    }
}

/// 包装 `semaphore_down`，返回是否发生过阻塞。
pub fn semaphore_down_contended(sem_id: usize) -> bool {
    match semaphore_down(sem_id) {
        0 => false,
        -1 => true,
        ret => panic!("unexpected semaphore_down return value: {ret}"),
    }
}

/// 包装 `condvar_wait`，返回是否发生过阻塞。
pub fn condvar_wait_contended(condvar_id: usize, mutex_id: usize) -> bool {
    match condvar_wait(condvar_id, mutex_id) {
        -1 => true,
        0 => false,
        ret => panic!("unexpected condvar_wait return value: {ret}"),
    }
}

fn update_max(target: &AtomicUsize, value: usize) {
    let mut current = target.load(Ordering::Relaxed);
    while value > current {
        match target.compare_exchange(current, value, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break,
            Err(actual) => current = actual,
        }
    }
}
