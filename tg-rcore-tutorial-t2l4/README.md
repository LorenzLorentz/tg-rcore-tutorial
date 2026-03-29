# t2l4：可插拔调度算法实验套件

这个目录现在不再是 `ch8` 的空壳拷贝，而是一个能直接跑实验的调度平台。

## 完成内容

我完成了下面几件核心工作：

1. 把原来固定的 FIFO ready queue 改成了可插拔调度器。
2. 在线程层补了统一的 `SchedEntity` 状态。
3. 做了统一的 trace / metric 收集。
4. 提供了四类 userland workload：
   - `cpu`
   - `io`
   - `interactive`
   - `mixed`
5. 提供了一站式核验脚本：
   - `./verify.sh`
   - `./test.sh` 只是它的别名

## 设计说明

### 1. 调度粒度

本实验以 **Thread** 为调度实体，而不是 Process。

原因很直接：

- 阻塞发生在线程层。
- 唤醒发生在线程层。
- 交互延迟也是“某个线程被唤醒后多久重新拿到 CPU”。

因此 `SchedEntity` 被放在 [`src/process.rs`](./src/process.rs) 的 `Thread` 里。

### 2. 调度器接口

内核里的策略层按下面这组语义组织：

- `enqueue(task)`
- `pick_next()`
- `on_tick()`
- `on_block()`
- `on_wakeup()`

具体实现位于 [`src/processor.rs`](./src/processor.rs)。

当前提供 5 种策略：

- `fcfs`
- `sjf`
- `rr`
- `mlfq`
- `cfs`

### 3. 为什么这里用了“虚拟 tick”

`ch8` 基线没有现成的 timer interrupt 抢占链路。如果这一章硬把真实时钟中断整条重建，任务规模会明显超出当前实验。

所以这里采用了一个明确写在 README 里的简化：

- 用户态 workload 通过 `lab_tick()` 主动发送一个轻量 trace syscall。
- 内核把这个请求映射到 `Scheduler::on_tick()`。

这样做的结果是：

- RR / MLFQ / CFS-like 仍然可以比较。
- FCFS / SJF 也能在同一套接口下运行。
- 实验重点仍然在“策略差异 + 统一观测”，不是把时间中断子系统整章重写一遍。

这个 `lab_tick()` 在 [`../tg-rcore-tutorial-user/src/lib.rs`](../tg-rcore-tutorial-user/src/lib.rs) 中提供。

## 统一观测口径

### Trace

每次上下文切换都会记录：

- 时间戳
- ready queue 长度
- 被切出的线程
- 被切入的线程
- 本次运行片段长度
- 事件类型

启用方式：

```bash
CHAPTER=9 T2L4_SCENARIO=cpu T2L4_SCHED=rr T2L4_TRACE=1 cargo run
```

输出中会出现：

- `[t2l4-summary] ...`
- `[t2l4-trace] ...`
- `[t2l4-task] ...`

### 指标定义

- `avg_wait_us`
  线程处于 ready 但未运行的累计等待时间均值。
- `avg_turnaround_us`
  完成时间减创建时间的均值。
- `throughput_milli_per_s`
  吞吐量，单位是“每秒任务数 x 1000”。
- `p95_latency_us` / `p99_latency_us`
  线程从 wakeup 到下一次真正获得 CPU 的延迟尾部指标。
- `starvation`
  线程等待超过阈值的次数统计。

为了避免 `no_std` 下的浮点格式化开销，内核 summary 统一输出整数；宿主侧脚本再把它换算成毫秒和每秒任务数。

## Workload 说明

workload 都放在 `tg-rcore-tutorial-user`：

- `sched_lab_cpu`
  多个纯计算线程，tick 长度不同，用来放大 FCFS / SJF / RR 差异。
- `sched_lab_io`
  线程频繁阻塞在 semaphore 上，由 producer 周期性唤醒。
- `sched_lab_interactive`
  更短的 burst、更密集的 wakeup，用来观察 P95 / P99 延迟。
- `sched_lab_mixed`
  CPU-heavy 线程和交互型线程混跑。

`initproc` 会在 `CHAPTER=9` 时根据 `T2L4_SCENARIO` 自动 `exec` 对应 workload。

## 一站式核验

### 推荐方式

直接在本目录执行：

```bash
./verify.sh
```

这个脚本会：

1. 如果当前不在 Docker 里，就自动通过 `~/rcore_docker.sh` 进入容器。
2. 运行完整矩阵：
   - 5 个 scheduler
   - 4 个 scenario
3. 抓取每次运行的 `[t2l4-summary]`
4. 在终端打印比较表
5. 把原始日志保存到 `.logs/suite/`

### 只跑子集

```bash
./verify.sh --scenarios cpu,interactive --schedulers fcfs,rr,cfs
```

### 调试单次运行

```bash
CHAPTER=9 T2L4_SCENARIO=cpu T2L4_SCHED=fcfs cargo run
```

如果需要看详细 trace：

```bash
CHAPTER=9 T2L4_SCENARIO=cpu T2L4_SCHED=fcfs T2L4_TRACE=1 cargo run
```

## 关键文件

- [`src/processor.rs`](./src/processor.rs)
  调度器、trace collector、`ProcessorInner`
- [`src/process.rs`](./src/process.rs)
  `SchedEntity` 和线程级状态
- [`src/main.rs`](./src/main.rs)
  trap/syscall 接入点，虚拟 tick 接线
- [`build.rs`](./build.rs)
  `t2l4` 用户程序打包逻辑
- [`verify.sh`](./verify.sh)
  一站式核验入口
- [`scripts/run_suite.py`](./scripts/run_suite.py)
  宿主侧矩阵执行与表格汇总
- [`lab-notes.md`](./lab-notes.md)
  过程记录

## 过程记录

我把“边做边想”的记录放在：

- [`lab-notes.md`](./lab-notes.md)

这份记录不是事后补摘要，而是按实现过程写的，包括：

- 为什么我没有直接重建 timer interrupt
- 为什么调度状态挂在线程里
- 我在收尾阶段踩到的报告输出 bug

## 已知简化

这里的 `CFS-like` 和 `SJF` 都是教学化简版本：

- `SJF`
  用指数平均保存 burst 估计，不偷看未来真实 burst。
- `CFS-like`
  用 `vruntime` 和线性 ready 集合，不实现 Linux 的红黑树和全部权重细节。
- `MLFQ`
  用固定层数与固定时间片数组，支持 wakeup boost，但没有把所有参数做成运行时配置文件。

这些简化都是刻意保留的：目标是让这章成为“可比较的实验平台”，不是把 Linux CFS 全量复刻进教程代码。
