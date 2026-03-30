use super::{Mutex, UPIntrFreeCell};
use alloc::{collections::VecDeque, sync::Arc};
use tg_task_manage::ThreadId;

/// 条件变量等待结果。
pub struct CondvarWaitResult {
    /// 因释放互斥锁而被唤醒的线程。
    pub mutex_wakeup_tid: Option<ThreadId>,
}

/// 条件变量 signal 结果。
pub struct CondvarSignalResult {
    /// 被 signal 的线程。
    pub tid: ThreadId,
    /// 该线程需要重新竞争的互斥锁 ID。
    pub mutex_id: usize,
    /// signal 时是否已经重新拿到互斥锁。
    pub acquired_mutex: bool,
}

/// 条件变量等待者。
pub struct CondvarWaiter {
    tid: ThreadId,
    mutex_id: usize,
    mutex: Arc<dyn Mutex>,
}

/// Condvar
pub struct Condvar {
    /// UPIntrFreeCell<CondvarInner>
    pub inner: UPIntrFreeCell<CondvarInner>,
}

/// CondvarInner
pub struct CondvarInner {
    /// block queue
    pub wait_queue: VecDeque<CondvarWaiter>,
}

impl Condvar {
    /// 创建一个新的条件变量。
    pub fn new() -> Self {
        Self {
            // SAFETY: 此条件变量仅在单处理器内核环境中使用
            inner: unsafe {
                UPIntrFreeCell::new(CondvarInner {
                    wait_queue: VecDeque::new(),
                })
            },
        }
    }
    /// 唤醒某个阻塞在当前条件变量上的线程
    pub fn signal(&self) -> Option<CondvarSignalResult> {
        let mut inner = self.inner.exclusive_access();
        inner.wait_queue.pop_front().map(|waiter| {
            let acquired_mutex = waiter.mutex.lock(waiter.tid);
            CondvarSignalResult {
                tid: waiter.tid,
                mutex_id: waiter.mutex_id,
                acquired_mutex,
            }
        })
    }

    /// 将当前线程阻塞在条件变量上
    pub fn wait_no_sched(&self, tid: ThreadId, mutex_id: usize, mutex: Arc<dyn Mutex>) -> bool {
        self.inner.exclusive_session(|inner| {
            inner.wait_queue.push_back(CondvarWaiter {
                tid,
                mutex_id,
                mutex,
            });
        });
        false
    }
    /// 原子地释放互斥锁并把线程挂到条件变量队列。
    ///
    /// 被 signal 的线程会在 signal 路径上重新竞争互斥锁，
    /// 只有真正拿到锁之后才会返回到用户态。
    pub fn wait_with_mutex(
        &self,
        tid: ThreadId,
        mutex_id: usize,
        mutex: Arc<dyn Mutex>,
    ) -> CondvarWaitResult {
        self.inner.exclusive_session(|inner| {
            inner.wait_queue.push_back(CondvarWaiter {
                tid,
                mutex_id,
                mutex: Arc::clone(&mutex),
            });
        });
        CondvarWaitResult {
            mutex_wakeup_tid: mutex.unlock(),
        }
    }
}
