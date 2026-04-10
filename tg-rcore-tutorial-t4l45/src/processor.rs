//! 处理器与调度实验平台。
//!
//! 本章不再把 ready queue 直接写死成一个 FIFO 容器，而是显式维护：
//! - 线程 / 进程实体表；
//! - 可插拔调度策略；
//! - 统一 trace collector；
//! - 线程级调度实体状态。
//!
//! 由于 `ch8` 基线没有真实时钟中断，本实验额外提供“虚拟 tick”：
//! 用户态 workload 通过一个轻量 trace syscall 主动打点，驱动
//! `on_tick()` 钩子，从而在 QEMU 中比较 FCFS / SJF / RR / MLFQ / CFS-like。

use crate::process::{Process, SchedEntity, Thread};
use alloc::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    vec::Vec,
};
use core::{
    cell::UnsafeCell,
    sync::atomic::{AtomicUsize, Ordering},
};
use spin::Mutex;
use tg_task_manage::{ProcId, ProcThreadRel, ThreadId};

const STARVATION_THRESHOLD_NS: u64 = 2_400_000;
const CFS_WAKEUP_BONUS_NS: u64 = 120_000;
pub const MAX_HARTS: usize = 4;

#[cfg(target_arch = "riscv64")]
pub fn current_hart() -> usize {
    let hart_id: usize;
    unsafe {
        core::arch::asm!("mv {}, tp", out(reg) hart_id);
    }
    hart_id
}

#[cfg(not(target_arch = "riscv64"))]
pub fn current_hart() -> usize {
    0
}

#[cfg(target_arch = "riscv64")]
fn time_now_ns() -> u64 {
    (riscv::register::time::read() as u64).saturating_mul(80)
}

#[cfg(not(target_arch = "riscv64"))]
fn time_now_ns() -> u64 {
    0
}

fn lab_scenario() -> &'static str {
    option_env!("T4L45_SCENARIO").unwrap_or("mixed")
}

fn trace_enabled() -> bool {
    matches!(
        option_env!("T4L45_TRACE"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

#[derive(Clone, Copy)]
enum TickDecision {
    Continue,
    Yield,
}

#[derive(Clone, Copy)]
enum SwitchReason {
    Start,
    Tick,
    Yield,
    Block,
    Exit,
    Wakeup,
}

impl SwitchReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Tick => "tick",
            Self::Yield => "yield",
            Self::Block => "block",
            Self::Exit => "exit",
            Self::Wakeup => "wakeup",
        }
    }
}

#[derive(Clone, Copy)]
struct SchedEvent {
    hart_id: usize,
    timestamp_ns: u64,
    queue_len: usize,
    prev_tid: Option<ThreadId>,
    next_tid: Option<ThreadId>,
    run_duration_ns: u64,
    reason: SwitchReason,
}

#[derive(Clone, Copy)]
struct PendingSwitch {
    hart_id: usize,
    timestamp_ns: u64,
    prev_tid: Option<ThreadId>,
    run_duration_ns: u64,
    reason: SwitchReason,
}

struct CompletedTaskStat {
    tid: ThreadId,
    created_at_ns: u64,
    finished_at_ns: u64,
    wait_ns: u64,
    turnaround_ns: u64,
    runtime_ns: u64,
    dispatch_count: usize,
    run_slices: usize,
    starvation_events: usize,
    interaction_latencies_ns: Vec<u64>,
}

struct SchedTraceCollector {
    scenario: &'static str,
    trace_enabled: bool,
    pending_switches: Vec<Option<PendingSwitch>>,
    events: Vec<SchedEvent>,
    completed: Vec<CompletedTaskStat>,
}

impl SchedTraceCollector {
    fn new() -> Self {
        Self {
            scenario: lab_scenario(),
            trace_enabled: trace_enabled(),
            pending_switches: alloc::vec![None; MAX_HARTS],
            events: Vec::new(),
            completed: Vec::new(),
        }
    }

    fn begin_switch(
        &mut self,
        hart_id: usize,
        prev_tid: Option<ThreadId>,
        reason: SwitchReason,
        run_duration_ns: u64,
        now_ns: u64,
    ) {
        if self.trace_enabled {
            self.pending_switches[hart_id] = Some(PendingSwitch {
                hart_id,
                timestamp_ns: now_ns,
                prev_tid,
                run_duration_ns,
                reason,
            });
        } else {
            self.pending_switches[hart_id] = None;
        }
    }

    fn complete_switch(
        &mut self,
        hart_id: usize,
        next_tid: Option<ThreadId>,
        queue_len: usize,
        now_ns: u64,
    ) {
        if self.trace_enabled {
            if let Some(pending) = self.pending_switches[hart_id].take() {
                self.events.push(SchedEvent {
                    hart_id: pending.hart_id,
                    timestamp_ns: pending.timestamp_ns,
                    queue_len,
                    prev_tid: pending.prev_tid,
                    next_tid,
                    run_duration_ns: pending.run_duration_ns,
                    reason: pending.reason,
                });
            } else if next_tid.is_some() {
                self.events.push(SchedEvent {
                    hart_id,
                    timestamp_ns: now_ns,
                    queue_len,
                    prev_tid: None,
                    next_tid,
                    run_duration_ns: 0,
                    reason: SwitchReason::Start,
                });
            }
        }
    }

    fn record_wakeup(&mut self, hart_id: usize, tid: ThreadId, queue_len: usize, now_ns: u64) {
        if self.trace_enabled {
            self.events.push(SchedEvent {
                hart_id,
                timestamp_ns: now_ns,
                queue_len,
                prev_tid: None,
                next_tid: Some(tid),
                run_duration_ns: 0,
                reason: SwitchReason::Wakeup,
            });
        }
    }

    fn push_completed(&mut self, tid: ThreadId, entity: SchedEntity) {
        self.completed.push(CompletedTaskStat {
            tid,
            created_at_ns: entity.created_at_ns,
            finished_at_ns: entity.finished_at_ns.unwrap_or(entity.created_at_ns),
            wait_ns: entity.total_wait_ns,
            turnaround_ns: entity.turnaround_ns(),
            runtime_ns: entity.total_runtime_ns,
            dispatch_count: entity.dispatch_count,
            run_slices: entity.run_slices,
            starvation_events: entity.starvation_events,
            interaction_latencies_ns: entity.interaction_latencies_ns,
        });
    }

    fn percentile_ns(samples: &mut [u64], p_num: usize, p_den: usize) -> u64 {
        if samples.is_empty() {
            return 0;
        }
        samples.sort_unstable();
        let idx = (samples.len() * p_num).div_ceil(p_den).saturating_sub(1);
        samples[idx]
    }

    fn print_report(&self, scheduler: SchedulerKind) {
        let context_switches = KERNEL_METRICS.context_switches.load(Ordering::Relaxed);
        if self.completed.is_empty() {
            println!(
                "[t4l45-sched-summary] scheduler={} scenario={} tasks=0 avg_wait_us=0 avg_turnaround_us=0 throughput_milli_per_s=0 p95_latency_us=0 p99_latency_us=0 starvation=0 ctx_switches={}",
                scheduler.as_str(),
                self.scenario,
                context_switches,
            );
            return;
        }

        let task_count = self.completed.len() as u64;
        let total_wait_ns: u64 = self.completed.iter().map(|task| task.wait_ns).sum();
        let total_turnaround_ns: u64 = self.completed.iter().map(|task| task.turnaround_ns).sum();
        let earliest_start_ns = self
            .completed
            .iter()
            .map(|task| task.created_at_ns)
            .min()
            .unwrap_or(0);
        let latest_finish_ns = self
            .completed
            .iter()
            .map(|task| task.finished_at_ns)
            .max()
            .unwrap_or(earliest_start_ns);
        let window_ns = latest_finish_ns.saturating_sub(earliest_start_ns).max(1);
        let mut latency_samples = self
            .completed
            .iter()
            .flat_map(|task| task.interaction_latencies_ns.iter().copied())
            .collect::<Vec<_>>();
        let mut latency_samples_p99 = latency_samples.clone();
        let p95_ns = Self::percentile_ns(&mut latency_samples, 95, 100);
        let p99_ns = Self::percentile_ns(&mut latency_samples_p99, 99, 100);
        let starvation_events: usize = self
            .completed
            .iter()
            .map(|task| task.starvation_events)
            .sum();
        let avg_wait_us = total_wait_ns / task_count / 1_000;
        let avg_turnaround_us = total_turnaround_ns / task_count / 1_000;
        let throughput_milli_per_s =
            ((task_count as u128) * 1_000_000_000_000u128 / window_ns as u128) as u64;
        let p95_us = p95_ns / 1_000;
        let p99_us = p99_ns / 1_000;

        println!(
            "[t4l45-sched-summary] scheduler={} scenario={} tasks={} avg_wait_us={} avg_turnaround_us={} throughput_milli_per_s={} p95_latency_us={} p99_latency_us={} starvation={} ctx_switches={}",
            scheduler.as_str(),
            self.scenario,
            task_count,
            avg_wait_us,
            avg_turnaround_us,
            throughput_milli_per_s,
            p95_us,
            p99_us,
            starvation_events,
            context_switches,
        );

        if self.trace_enabled {
            for event in &self.events {
                println!(
                    "[t4l45-sched-trace] hart={} ts_ns={} reason={} prev_tid={} next_tid={} qlen={} slice_ns={}",
                    event.hart_id,
                    event.timestamp_ns,
                    event.reason.as_str(),
                    event
                        .prev_tid
                        .map(|tid| tid.get_usize())
                        .unwrap_or(usize::MAX),
                    event
                        .next_tid
                        .map(|tid| tid.get_usize())
                        .unwrap_or(usize::MAX),
                    event.queue_len,
                    event.run_duration_ns
                );
            }
            for task in &self.completed {
                println!(
                    "[t4l45-sched-task] tid={} wait_us={} turnaround_us={} runtime_us={} dispatches={} slices={} starvation={}",
                    task.tid.get_usize(),
                    task.wait_ns / 1_000,
                    task.turnaround_ns / 1_000,
                    task.runtime_ns / 1_000,
                    task.dispatch_count,
                    task.run_slices,
                    task.starvation_events
                );
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum SchedulerKind {
    Fcfs,
    Sjf,
    Rr,
    Mlfq,
    Cfs,
}

impl SchedulerKind {
    fn from_env() -> Self {
        match option_env!("T4L45_SCHED").unwrap_or("rr") {
            "fcfs" | "FCFS" => Self::Fcfs,
            "sjf" | "SJF" => Self::Sjf,
            "rr" | "RR" => Self::Rr,
            "mlfq" | "MLFQ" => Self::Mlfq,
            "cfs" | "CFS" => Self::Cfs,
            _ => Self::Rr,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Fcfs => "fcfs",
            Self::Sjf => "sjf",
            Self::Rr => "rr",
            Self::Mlfq => "mlfq",
            Self::Cfs => "cfs",
        }
    }
}

struct FifoState {
    ready: VecDeque<ThreadId>,
}

struct SjfState {
    ready: Vec<ThreadId>,
}

struct RrState {
    ready: VecDeque<ThreadId>,
    quantum_ticks: u32,
}

struct MlfqState {
    queues: Vec<VecDeque<ThreadId>>,
}

struct CfsState {
    ready: Vec<ThreadId>,
}

enum SchedulerCore {
    Fcfs(FifoState),
    Sjf(SjfState),
    Rr(RrState),
    Mlfq(MlfqState),
    Cfs(CfsState),
}

impl SchedulerCore {
    const MLFQ_QUANTA: [u32; 4] = [1, 2, 4, 8];

    fn new(kind: SchedulerKind) -> Self {
        match kind {
            SchedulerKind::Fcfs => Self::Fcfs(FifoState {
                ready: VecDeque::new(),
            }),
            SchedulerKind::Sjf => Self::Sjf(SjfState { ready: Vec::new() }),
            SchedulerKind::Rr => Self::Rr(RrState {
                ready: VecDeque::new(),
                quantum_ticks: 2,
            }),
            SchedulerKind::Mlfq => Self::Mlfq(MlfqState {
                queues: (0..Self::MLFQ_QUANTA.len())
                    .map(|_| VecDeque::new())
                    .collect::<Vec<_>>(),
            }),
            SchedulerKind::Cfs => Self::Cfs(CfsState { ready: Vec::new() }),
        }
    }

    fn len(&self) -> usize {
        match self {
            Self::Fcfs(state) => state.ready.len(),
            Self::Sjf(state) => state.ready.len(),
            Self::Rr(state) => state.ready.len(),
            Self::Mlfq(state) => state.queues.iter().map(VecDeque::len).sum(),
            Self::Cfs(state) => state.ready.len(),
        }
    }

    fn remove_ready(&mut self, tid: ThreadId) {
        match self {
            Self::Fcfs(state) => {
                if let Some(idx) = state.ready.iter().position(|&item| item == tid) {
                    state.ready.remove(idx);
                }
            }
            Self::Sjf(state) => {
                if let Some(idx) = state.ready.iter().position(|&item| item == tid) {
                    state.ready.remove(idx);
                }
            }
            Self::Rr(state) => {
                if let Some(idx) = state.ready.iter().position(|&item| item == tid) {
                    state.ready.remove(idx);
                }
            }
            Self::Mlfq(state) => {
                for queue in &mut state.queues {
                    if let Some(idx) = queue.iter().position(|&item| item == tid) {
                        queue.remove(idx);
                        break;
                    }
                }
            }
            Self::Cfs(state) => {
                if let Some(idx) = state.ready.iter().position(|&item| item == tid) {
                    state.ready.remove(idx);
                }
            }
        }
    }

    fn enqueue(&mut self, tid: ThreadId, entity: &SchedEntity) {
        self.remove_ready(tid);
        match self {
            Self::Fcfs(state) => state.ready.push_back(tid),
            Self::Sjf(state) => state.ready.push(tid),
            Self::Rr(state) => state.ready.push_back(tid),
            Self::Mlfq(state) => {
                let level = entity.queue_level.min(state.queues.len().saturating_sub(1));
                state.queues[level].push_back(tid);
            }
            Self::Cfs(state) => state.ready.push(tid),
        }
    }

    fn pick_next(&mut self, threads: &BTreeMap<ThreadId, ThreadRecord>) -> Option<ThreadId> {
        match self {
            Self::Fcfs(state) => state.ready.pop_front(),
            Self::Sjf(state) => {
                let (idx, _) = state.ready.iter().enumerate().min_by_key(|(_, tid)| {
                    let record = threads.get(tid).unwrap();
                    (record.thread.sched.burst_estimate_ns, tid.get_usize())
                })?;
                Some(state.ready.remove(idx))
            }
            Self::Rr(state) => state.ready.pop_front(),
            Self::Mlfq(state) => state.queues.iter_mut().find_map(|queue| queue.pop_front()),
            Self::Cfs(state) => {
                let (idx, _) = state.ready.iter().enumerate().min_by_key(|(_, tid)| {
                    let record = threads.get(tid).unwrap();
                    (record.thread.sched.vruntime_ns, tid.get_usize())
                })?;
                Some(state.ready.remove(idx))
            }
        }
    }

    fn account_runtime(&mut self, entity: &mut SchedEntity, delta_ns: u64) {
        if let Self::Cfs(_) = self {
            let weight = entity.weight.max(1);
            entity.vruntime_ns = entity
                .vruntime_ns
                .saturating_add(delta_ns.saturating_mul(SchedEntity::DEFAULT_WEIGHT) / weight);
        }
    }

    fn on_tick(&mut self, entity: &mut SchedEntity, delta_ns: u64) -> TickDecision {
        self.account_runtime(entity, delta_ns);
        match self {
            Self::Fcfs(_) => TickDecision::Continue,
            Self::Sjf(_) => TickDecision::Yield,
            Self::Rr(state) => {
                entity.tick_budget_used = entity.tick_budget_used.saturating_add(1);
                if entity.tick_budget_used >= state.quantum_ticks {
                    entity.tick_budget_used = 0;
                    TickDecision::Yield
                } else {
                    TickDecision::Continue
                }
            }
            Self::Mlfq(_) => {
                entity.tick_budget_used = entity.tick_budget_used.saturating_add(1);
                let quantum = Self::MLFQ_QUANTA
                    .get(entity.queue_level)
                    .copied()
                    .unwrap_or(*Self::MLFQ_QUANTA.last().unwrap());
                if entity.tick_budget_used >= quantum {
                    entity.tick_budget_used = 0;
                    entity.queue_level =
                        (entity.queue_level + 1).min(Self::MLFQ_QUANTA.len().saturating_sub(1));
                    TickDecision::Yield
                } else {
                    TickDecision::Continue
                }
            }
            Self::Cfs(_) => TickDecision::Yield,
        }
    }

    fn on_yield(&mut self, entity: &mut SchedEntity) {
        entity.tick_budget_used = 0;
    }

    fn on_block(&mut self, entity: &mut SchedEntity) {
        match self {
            Self::Mlfq(_) => {
                if entity.tick_budget_used <= 1 {
                    entity.queue_level = entity.queue_level.saturating_sub(1);
                }
                entity.tick_budget_used = 0;
            }
            Self::Cfs(_) | Self::Fcfs(_) | Self::Sjf(_) | Self::Rr(_) => {
                entity.tick_budget_used = 0;
            }
        }
    }

    fn on_wakeup(&mut self, entity: &mut SchedEntity) {
        match self {
            Self::Mlfq(_) => {
                entity.queue_level = entity.queue_level.saturating_sub(1);
            }
            Self::Cfs(_) => {
                entity.vruntime_ns = entity.vruntime_ns.saturating_sub(CFS_WAKEUP_BONUS_NS);
            }
            Self::Fcfs(_) | Self::Sjf(_) | Self::Rr(_) => {}
        }
    }
}

/// 处理器内部状态。
///
/// 该结构把“线程/进程关系 + 调度策略 + 统一观测”集中在一个地方，
/// 以便在主调度循环中用统一入口驱动各类调度算法。
struct ThreadRecord {
    pid: ProcId,
    thread: Thread,
}

pub struct RunningTask {
    pub pid: ProcId,
    pub thread: Thread,
    pub started_at_ns: u64,
    pub accounted_at_ns: u64,
}

impl RunningTask {
    fn new(pid: ProcId, thread: Thread, now_ns: u64) -> Self {
        Self {
            pid,
            thread,
            started_at_ns: now_ns,
            accounted_at_ns: now_ns,
        }
    }
}

pub struct ProcessorInner {
    rel_map: BTreeMap<ProcId, ProcThreadRel>,
    procs: BTreeMap<ProcId, Process>,
    threads: BTreeMap<ThreadId, ThreadRecord>,
    tid2pid: BTreeMap<ThreadId, ProcId>,
    pending_wakeups: BTreeSet<ThreadId>,
    scheduler_kind: SchedulerKind,
    scheduler: SchedulerCore,
    trace: SchedTraceCollector,
}

impl ProcessorInner {
    /// 创建新的处理器内部状态。
    pub fn new() -> Self {
        let scheduler_kind = SchedulerKind::from_env();
        Self {
            rel_map: BTreeMap::new(),
            procs: BTreeMap::new(),
            threads: BTreeMap::new(),
            tid2pid: BTreeMap::new(),
            pending_wakeups: BTreeSet::new(),
            scheduler_kind,
            scheduler: SchedulerCore::new(scheduler_kind),
            trace: SchedTraceCollector::new(),
        }
    }

    fn account_running(&mut self, task: &mut RunningTask, reason: SwitchReason, hart_id: usize) {
        let tid = task.thread.tid;
        let now_ns = time_now_ns();
        let residual_ns = now_ns.saturating_sub(task.accounted_at_ns);
        if residual_ns > 0 {
            self.scheduler
                .account_runtime(&mut task.thread.sched, residual_ns);
        }
        let burst_ns = now_ns.saturating_sub(task.started_at_ns);
        task.thread.sched.record_run(burst_ns);
        match reason {
            SwitchReason::Block => self.scheduler.on_block(&mut task.thread.sched),
            SwitchReason::Tick | SwitchReason::Yield => {
                self.scheduler.on_yield(&mut task.thread.sched)
            }
            SwitchReason::Exit | SwitchReason::Start | SwitchReason::Wakeup => {}
        }
        self.trace
            .begin_switch(hart_id, Some(tid), reason, burst_ns, now_ns);
    }

    /// 处理一个或多个调度 tick，返回 `true` 表示需要发生调度切换。
    pub fn handle_ticks(&mut self, task: &mut RunningTask, ticks: usize) -> bool {
        if ticks == 0 {
            return false;
        }
        let now_ns = time_now_ns();
        let delta_ns = now_ns.saturating_sub(task.accounted_at_ns);
        task.accounted_at_ns = now_ns;
        let mut should_yield = false;
        for idx in 0..ticks {
            let tick_delta = if idx == 0 { delta_ns } else { 0 };
            should_yield |= matches!(
                self.scheduler.on_tick(&mut task.thread.sched, tick_delta),
                TickDecision::Yield
            );
        }
        should_yield
    }

    /// 处理单个调度 tick，返回 `true` 表示需要发生调度切换。
    pub fn handle_tick(&mut self, task: &mut RunningTask) -> bool {
        self.handle_ticks(task, 1)
    }

    /// 查找当前应运行的线程。
    pub fn take_next(&mut self, hart_id: usize) -> Option<RunningTask> {
        let now_ns = time_now_ns();
        let next_tid = self.scheduler.pick_next(&self.threads);
        self.trace
            .complete_switch(hart_id, next_tid, self.scheduler.len(), now_ns);
        let next_tid = if let Some(next_tid) = next_tid {
            next_tid
        } else {
            if self.trace.trace_enabled {
                println!("[t2l4-kernel] ready queue empty");
            }
            return None;
        };
        KERNEL_METRICS
            .context_switches
            .fetch_add(1, Ordering::Relaxed);
        let ThreadRecord { pid, mut thread } = self.threads.remove(&next_tid).unwrap();
        thread.sched.on_dispatch(now_ns, STARVATION_THRESHOLD_NS);
        Some(RunningTask::new(pid, thread, now_ns))
    }

    /// 把当前线程按普通让出 CPU 处理。
    pub fn make_running_suspend(&mut self, hart_id: usize, mut task: RunningTask) {
        self.account_running(&mut task, SwitchReason::Yield, hart_id);
        let tid = task.thread.tid;
        let now_ns = time_now_ns();
        task.thread.sched.on_ready(now_ns);
        self.scheduler.enqueue(tid, &task.thread.sched);
        self.threads.insert(
            tid,
            ThreadRecord {
                pid: task.pid,
                thread: task.thread,
            },
        );
    }

    /// 把当前线程按 tick 抢占处理。
    pub fn make_running_tick_suspend(&mut self, hart_id: usize, mut task: RunningTask) {
        self.account_running(&mut task, SwitchReason::Tick, hart_id);
        let tid = task.thread.tid;
        let now_ns = time_now_ns();
        task.thread.sched.on_ready(now_ns);
        self.scheduler.enqueue(tid, &task.thread.sched);
        self.threads.insert(
            tid,
            ThreadRecord {
                pid: task.pid,
                thread: task.thread,
            },
        );
    }

    /// 把当前线程标记为阻塞。
    pub fn make_running_blocked(&mut self, hart_id: usize, mut task: RunningTask) {
        let tid = task.thread.tid;
        self.account_running(&mut task, SwitchReason::Block, hart_id);
        let was_woken_early = self.pending_wakeups.remove(&tid);
        self.threads.insert(
            tid,
            ThreadRecord {
                pid: task.pid,
                thread: task.thread,
            },
        );
        if was_woken_early {
            let now_ns = time_now_ns();
            if let Some(record) = self.threads.get_mut(&tid) {
                record.thread.sched.on_wakeup(now_ns);
                self.scheduler.on_wakeup(&mut record.thread.sched);
                self.scheduler.enqueue(tid, &record.thread.sched);
            }
        }
    }

    /// 结束当前线程。
    pub fn make_running_exited(&mut self, hart_id: usize, mut task: RunningTask, exit_code: isize) {
        let tid = task.thread.tid;
        self.account_running(&mut task, SwitchReason::Exit, hart_id);
        self.pending_wakeups.remove(&tid);
        let now_ns = time_now_ns();
        task.thread.sched.on_finish(now_ns);
        self.trace.push_completed(tid, task.thread.sched);
        let pid = self.tid2pid.remove(&tid).unwrap();
        let mut delete_proc = false;
        if let Some(rel) = self.rel_map.get_mut(&pid) {
            rel.del_thread(tid, exit_code);
            if rel.threads.is_empty() {
                delete_proc = true;
            }
        }
        if delete_proc {
            self.del_proc(pid, exit_code);
        }
        if self.trace.trace_enabled {
            let remaining_threads = self
                .rel_map
                .get(&pid)
                .map(|rel| rel.threads.len())
                .unwrap_or(0);
            println!(
                "[t2l4-kernel] exit tid={} remaining_threads={}",
                tid.get_usize(),
                remaining_threads
            );
        }
    }

    /// 唤醒某个阻塞线程并重新入队。
    pub fn re_enque(&mut self, hart_id: usize, tid: ThreadId) {
        let now_ns = time_now_ns();
        if let Some(record) = self.threads.get_mut(&tid) {
            record.thread.sched.on_wakeup(now_ns);
            self.scheduler.on_wakeup(&mut record.thread.sched);
            self.scheduler.enqueue(tid, &record.thread.sched);
            self.trace
                .record_wakeup(hart_id, tid, self.scheduler.len(), now_ns);
        } else {
            // SMP 下可能出现“先被唤醒、后真正写回 blocked 表”的交错。
            // 这种情况下先记下唤醒，等 make_running_blocked() 完成落表后再入队。
            self.pending_wakeups.insert(tid);
            self.trace
                .record_wakeup(hart_id, tid, self.scheduler.len(), now_ns);
        }
    }

    /// 添加线程到当前进程并立刻进入 ready 队列。
    pub fn add(&mut self, tid: ThreadId, mut thread: Thread, pid: ProcId) {
        let now_ns = time_now_ns();
        thread.sched.on_created(now_ns);
        self.threads.insert(tid, ThreadRecord { pid, thread });
        self.scheduler
            .enqueue(tid, &self.threads.get(&tid).unwrap().thread.sched);
        if let Some(rel) = self.rel_map.get_mut(&pid) {
            rel.add_thread(tid);
        }
        self.tid2pid.insert(tid, pid);
    }

    /// 根据 TID 获取线程。
    pub fn get_task(&mut self, tid: ThreadId) -> Option<&mut Thread> {
        self.threads.get_mut(&tid).map(|record| &mut record.thread)
    }

    /// 添加进程。
    pub fn add_proc(&mut self, pid: ProcId, proc: Process, parent: ProcId) {
        self.procs.insert(pid, proc);
        if let Some(parent_rel) = self.rel_map.get_mut(&parent) {
            parent_rel.add_child(pid);
        }
        self.rel_map.insert(pid, ProcThreadRel::new(parent));
    }

    /// 获取进程。
    pub fn get_proc(&mut self, pid: ProcId) -> Option<&mut Process> {
        self.procs.get_mut(&pid)
    }

    /// 获取某个进程的线程列表。
    pub fn get_thread(&mut self, pid: ProcId) -> Option<&Vec<ThreadId>> {
        self.rel_map.get(&pid).map(|rel| &rel.threads)
    }

    /// 删除进程并维护父子关系。
    pub fn del_proc(&mut self, pid: ProcId, exit_code: isize) {
        self.procs.remove(&pid);
        let rel = self.rel_map.remove(&pid).unwrap();
        let parent_pid = rel.parent;
        let children = rel.children;
        if let Some(parent_rel) = self.rel_map.get_mut(&parent_pid) {
            parent_rel.del_child(pid, exit_code);
        }
        for child in children {
            self.rel_map.get_mut(&child).unwrap().parent = ProcId::from_usize(0);
            self.rel_map
                .get_mut(&ProcId::from_usize(0))
                .unwrap()
                .add_child(child);
        }
    }

    /// wait 系统调用。
    pub fn wait(&mut self, pid: ProcId, child_pid: ProcId) -> Option<(ProcId, isize)> {
        self.rel_map
            .get_mut(&pid)
            .unwrap()
            .wait_child(child_pid)
            .or_else(|| {
                if child_pid.get_usize() == usize::MAX {
                    self.rel_map.get_mut(&pid).unwrap().wait_any_child()
                } else {
                    None
                }
            })
    }

    /// waittid 系统调用。
    pub fn waittid(&mut self, pid: ProcId, tid: ThreadId) -> Option<isize> {
        self.rel_map.get_mut(&pid).unwrap().wait_thread(tid)
    }

    pub fn is_drained(&self) -> bool {
        self.threads.is_empty()
    }

    /// 输出实验报告。
    pub fn print_report(&self) {
        self.trace.print_report(self.scheduler_kind);
    }
}

/// 内核同步实验计数器。
pub struct KernelMetrics {
    /// 总上下文切换次数。
    pub context_switches: AtomicUsize,
    /// 因同步原语阻塞的次数。
    pub blocked_sync_ops: AtomicUsize,
    /// 同步原语唤醒次数。
    pub wakeups: AtomicUsize,
}

impl KernelMetrics {
    /// 创建空计数器。
    pub const fn new() -> Self {
        Self {
            context_switches: AtomicUsize::new(0),
            blocked_sync_ops: AtomicUsize::new(0),
            wakeups: AtomicUsize::new(0),
        }
    }

    /// 清零计数器。
    pub fn reset(&self) {
        self.context_switches.store(0, Ordering::Relaxed);
        self.blocked_sync_ops.store(0, Ordering::Relaxed);
        self.wakeups.store(0, Ordering::Relaxed);
    }
}

/// 全局处理器包装。
#[derive(Clone, Copy)]
struct CurrentTaskRef {
    pid: ProcId,
    tid: ThreadId,
    thread_ptr: *mut Thread,
}

struct CurrentSlot {
    current: Option<CurrentTaskRef>,
}

impl CurrentSlot {
    const fn new() -> Self {
        Self { current: None }
    }
}

pub struct Processor {
    inner: Mutex<Option<ProcessorInner>>,
    current: [UnsafeCell<CurrentSlot>; MAX_HARTS],
    running_tasks: AtomicUsize,
}

unsafe impl Sync for Processor {}

impl Processor {
    /// 创建一个尚未初始化的处理器。
    pub const fn new() -> Self {
        Self {
            inner: Mutex::new(None),
            current: [const { UnsafeCell::new(CurrentSlot::new()) }; MAX_HARTS],
            running_tasks: AtomicUsize::new(0),
        }
    }

    /// 初始化处理器。
    pub fn init(&self, inner: ProcessorInner) {
        {
            let mut guard = self.inner.lock();
            *guard = Some(inner);
        }
        self.running_tasks.store(0, Ordering::Relaxed);
        for hart_id in 0..MAX_HARTS {
            unsafe {
                (*self.current[hart_id].get()).current = None;
            }
        }
    }

    fn slot(&self, hart_id: usize) -> &mut CurrentSlot {
        assert!(hart_id < MAX_HARTS);
        unsafe { &mut *self.current[hart_id].get() }
    }

    pub fn with_inner<R>(&self, f: impl FnOnce(&mut ProcessorInner) -> R) -> R {
        let mut guard = self.inner.lock();
        f(guard.as_mut().unwrap())
    }

    pub fn take_next(&self, hart_id: usize) -> Option<RunningTask> {
        let mut guard = self.inner.lock();
        let inner = guard.as_mut().unwrap();
        let task = inner.take_next(hart_id);
        if task.is_some() {
            self.running_tasks.fetch_add(1, Ordering::Relaxed);
        }
        task
    }

    pub fn install_current(&self, hart_id: usize, pid: ProcId, thread: &mut Thread) {
        let slot = self.slot(hart_id);
        if slot.current.take().is_some() {
            // `current` 只是一份 per-hart 运行中镜像；若上一条路径已经把线程状态
            // 写回调度器但遗漏了退休这份镜像，这里回收旧记录以避免把调试构建直接打崩。
            self.running_tasks.fetch_sub(1, Ordering::Relaxed);
        }
        slot.current = Some(CurrentTaskRef {
            pid,
            tid: thread.tid,
            thread_ptr: thread as *mut Thread,
        });
    }

    pub fn clear_current(&self, hart_id: usize) {
        let slot = self.slot(hart_id);
        if slot.current.take().is_some() {
            self.running_tasks.fetch_sub(1, Ordering::Relaxed);
        }
    }

    pub fn current_tid(&self, hart_id: usize) -> Option<ThreadId> {
        self.slot(hart_id).current.map(|current| current.tid)
    }

    pub fn current_pid(&self, hart_id: usize) -> Option<ProcId> {
        self.slot(hart_id).current.map(|current| current.pid)
    }

    pub fn with_current_thread<R>(
        &self,
        hart_id: usize,
        f: impl FnOnce(&mut Thread) -> R,
    ) -> Option<R> {
        let current = self.slot(hart_id).current?;
        Some(f(unsafe { &mut *current.thread_ptr }))
    }

    pub fn with_current_proc<R>(
        &self,
        hart_id: usize,
        f: impl FnOnce(&mut Process) -> R,
    ) -> Option<R> {
        let pid = self.current_pid(hart_id)?;
        Some(self.with_inner(|inner| f(inner.procs.get_mut(&pid).unwrap())))
    }

    pub fn with_current_proc_and_thread<R>(
        &self,
        hart_id: usize,
        f: impl FnOnce(&mut Process, &mut Thread) -> R,
    ) -> Option<R> {
        let pid = self.current_pid(hart_id)?;
        let thread_ptr = self.slot(hart_id).current?.thread_ptr;
        let mut guard = self.inner.lock();
        let inner = guard.as_mut().unwrap();
        let proc = inner.procs.get_mut(&pid).unwrap();
        Some(f(proc, unsafe { &mut *thread_ptr }))
    }

    pub fn with_proc<R>(&self, pid: ProcId, f: impl FnOnce(&mut Process) -> R) -> Option<R> {
        self.with_inner(|inner| inner.procs.get_mut(&pid).map(f))
    }

    pub fn running_tasks(&self) -> usize {
        self.running_tasks.load(Ordering::Relaxed)
    }
}

/// 全局处理器实例。
pub static PROCESSOR: Processor = Processor::new();

/// 全局内核同步实验计数器。
pub static KERNEL_METRICS: KernelMetrics = KernelMetrics::new();
