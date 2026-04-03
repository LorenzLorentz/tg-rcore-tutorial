# t4l45：调度 + 同步联合实验基线

`tg-rcore-tutorial-t4l45` 不是新的空目录，也不是简单把 `t2l4` 和 `t2l5` 拼在一起。

它现在承担两个角色：

1. 作为当前阶段可运行、可测量、可回归的 **联合实验基线**。
2. 作为后续两项更大改造的 **起点**：
   - `T4L45` 支持内核响应中断
   - `T4L45` 支持多核处理和多核调度

这份 README 的目的不是重复讲一遍实现细节，而是给后续继续开发的人一个清晰的 guidance：

- 现在已经有什么
- 现在还故意没有什么
- 接下来应该先做什么，再做什么

## 当前已经实现了什么

### 1. 线程级可插拔调度器

当前调度实体是 **Thread**，不是 Process。

已经实现并统一接入的策略：

- `fcfs`
- `sjf`
- `rr`
- `mlfq`
- `cfs`（教学化简版）

统一接口语义在 [`src/processor.rs`](./src/processor.rs) 中体现为：

- `enqueue`
- `pick_next`
- `on_tick`
- `on_block`
- `on_wakeup`

线程调度状态 `SchedEntity` 仍然挂在 [`src/process.rs`](./src/process.rs) 的 `Thread` 上，因为等待、阻塞、唤醒、交互延迟天然都是线程级行为。

### 2. 调度统一观测与报告

当前内核已经能输出统一调度报告：

- 平均等待时间
- 平均周转时间
- 吞吐量
- P95 / P99 交互延迟
- 饥饿事件数
- 上下文切换次数

默认 summary 前缀是：

- `[t4l45-sched-summary]`

在显式开启 `T4L45_TRACE=1` 时，还会输出：

- `[t4l45-sched-trace]`
- `[t4l45-sched-task]`

实现位置：

- [`src/processor.rs`](./src/processor.rs)

### 3. 同步原语实验能力

当前 `t4l45` 已经合入 `t2l5` 的同步实验语义和计数器，包括：

- 用户态 `ticket spinlock`
- 内核阻塞式 `mutex`
- `semaphore`
- `condvar`
- 用户态 `fair rwlock`
- 用户态 `reader-prefer rwlock`

同步相关观测口径包括：

- `ctx_switches`
- `blocked`
- `wakeups`
- `avg_wait_us`
- `max_wait_us`
- `avg_hold_us`
- `max_hold_us`
- `starvation`

同步语义里，已经保留并验证了几个关键点：

- `mutex` 的 handoff 唤醒路径
- `semaphore` 的阻塞 / 唤醒路径
- `condvar_wait` 释放 mutex 后进入等待队列
- `condvar_signal` 在内核里帮助 waiter 重新竞争 mutex
- `fault mode` 注入：
  - `mutex_drop_wakeup`
  - `semaphore_drop_wakeup`

主要接入点在 [`src/main.rs`](./src/main.rs)。

### 4. 统一用户态入口与联合镜像

`initproc` 已经支持 `CHAPTER=45`，并根据 `T4L45_SCENARIO` 分发 workload。

见：

- [`../tg-rcore-tutorial-user/src/bin/initproc.rs`](../tg-rcore-tutorial-user/src/bin/initproc.rs)
- [`../tg-rcore-tutorial-user/cases.toml`](../tg-rcore-tutorial-user/cases.toml)

当前 `t4l45` 镜像已经同时打包：

- 原 `t2l4` 的调度 workload
- 原 `t2l5` 的同步 workload
- 新增的复杂压力 workload

### 5. 基础测试已经合并

当前测试已经统一进一个 suite：

- `scheduler`
  调度矩阵：`4 scenario x 5 scheduler`
- `sync`
  同步基础成功用例
- `control`
  故障注入与错误写法对照组
- `robust`
  跨调度器同步压力测试 + 更复杂的混合压力测试

入口：

- [`test.sh`](./test.sh)
- [`verify.sh`](./verify.sh)
- [`scripts/run_suite.py`](./scripts/run_suite.py)

### 6. 新增了更复杂的鲁棒性 / 性能测试

相比原来的 `t2l4` / `t2l5`，现在额外增加了两类联合压力：

- [`t4l45_hybrid_pipeline.rs`](../tg-rcore-tutorial-user/src/bin/t4l45_hybrid_pipeline.rs)
  `mutex + condvar + producer/consumer + CPU hog + lab_tick`
- [`t4l45_semaphore_ring.rs`](../tg-rcore-tutorial-user/src/bin/t4l45_semaphore_ring.rs)
  长链 handoff 的 semaphore ring

它们的目的不是替代基础测试，而是补上“调度和同步同时受压”的场景。

## 当前刻意保留的边界

这个版本虽然已经是联合 baseline，但它仍然有两个非常重要的边界。

### 1. 还没有真正的内核中断驱动调度

现在的 `on_tick()` 仍然依赖用户态主动调用 `lab_tick()`。

这意味着：

- 它是一个 **虚拟 tick**
- 它适合比较调度策略
- 但它还不是“内核被真实 timer interrupt 抢占后强制调度”

也就是说，`t4l45` 当前是：

- 有调度策略
- 有调度观测
- 有同步阻塞 / 唤醒
- 但还没有“由真实中断驱动的抢占”

### 2. 还是假设单核

当前很多实现仍然带着明显的 UP 假设：

- 当前执行流只有一个 hart
- 当前 `PROCESSOR` 只有一个全局 current
- 调度器只维护一套 ready 结构
- `tg-sync` 里的很多内部假设也是“单处理器 + 关中断即可保护临界区”

所以它现在是一个 **单核联合实验基线**，还不是 SMP 基线。

## 它将要实现什么

后续明确有两个方向。

### 方向一：让 T4L45 支持内核响应中断

这一步的目标不是“加几个中断号”，而是把当前的调度时钟源从“用户主动打点”升级成“内核真实响应异步事件”。

应当达到的效果：

- 内核能正确进入中断 trap 路径
- timer interrupt 能驱动调度时钟前进
- 抢占不再依赖用户主动 `lab_tick()`
- 外设中断 / 软件中断至少有清晰的处理框架
- 同步阻塞 / 唤醒与中断路径不会互相破坏

这一步完成后，`lab_tick()` 最好退化成：

- 调试辅助工具
- 对照实验工具

而不是系统正确性的唯一前提。

### 方向二：让 T4L45 支持多核处理和多核调度

这一步的目标不是“把 `CPU_NUM=2` 写进配置”，而是让当前单核实验平台真正迈向 SMP。

应当达到的效果：

- 多个 hart 能启动并进入内核
- 每个 hart 都有自己的当前执行上下文
- 调度器能在多核场景下选择任务
- 就绪队列、阻塞队列、唤醒路径具备并发安全性
- 跨核唤醒、负载均衡、可能的 IPI 路径有清晰方案
- 同步原语不再依赖 UP-only 假设

## 推荐的推进顺序

后续继续开发时，建议按下面顺序推进，而不要把“中断”和“多核”同时硬上。

### 第 1 步：先把真实中断接进单核

优先目标：

- 在单核下打通 timer interrupt trap 路径
- 把 `Scheduler::on_tick()` 改成由内核时钟中断驱动
- 保留 `lab_tick()` 作为调试辅助，而不是主路径
- 确保现有 `scheduler/sync/control` 基础测试不回归

原因很简单：

- 如果单核中断路径都没稳定，多核只会把问题放大
- 很多调度器状态推进逻辑，应该先在“单核 + 真实时钟”下站稳

### 第 2 步：识别并清理 UP 假设

在进入多核前，先系统清点以下内容：

- 哪些锁只对单核成立
- 哪些数据结构默认只有一个执行者
- 哪些“关中断保护临界区”的写法在 SMP 下失效
- 哪些 wakeup 路径默认“唤醒者和被唤醒者在同一核”

这一步最好形成一个明确清单，而不是边做边猜。

### 第 3 步：再引入多核调度结构

到这一步再决定下面这些问题：

- 全局 ready queue 还是 per-core ready queue
- 是否需要 work stealing
- 如何做跨核 wakeup
- 如何统计 per-core metrics 与 global metrics
- 抢占发生时，`current` 状态放在哪里

不要在第一步就提前锁死所有架构选择。

## 对后续实现最重要的设计约束

### 1. 不要破坏当前 baseline 的可测性

后续做中断和多核时，最容易丢掉的不是功能，而是“统一观测口径”。

应当尽量保留当前这些输出接口：

- 调度 summary
- 同步 summary
- control marker
- robust suite

因为它们是后续判断回归和行为变化的基础。

### 2. 不要把“调度正确性”和“测量逻辑”完全耦死

当前已经有比较明确的分层：

- 调度策略
- trap / syscall / block / wakeup 驱动
- 统计与 trace collector

后续改成真实中断、多核后，也尽量保持这个分层，不要让 collector 反向干扰调度路径。

### 3. 先保证正确，再扩复杂策略

在进入中断 / 多核阶段后，最值得优先保住的是：

- `rr`
- `mutex/semaphore/condvar`
- 基础阻塞 / 唤醒

`mlfq` 和 `cfs-like` 可以后续再精修。

原因不是它们不重要，而是它们对时钟源、优先级推进、跨核公平性更敏感，适合在基础路径稳定后再扩。

## 当前测试策略的建议用法

### 日常回归

```bash
./test.sh base
./verify.sh --mode scheduler
./verify.sh --mode sync
./verify.sh --mode control
```

### 做中断改造时

重点先看：

- `scheduler`
- `sync`
- `control`

因为这三类最能快速暴露：

- tick 驱动错了
- 抢占点错了
- block / wakeup 语义被破坏了

### 做多核改造时

重点再看：

- `sync`
- `robust`

因为多核下真正先坏掉的通常不是“单次功能”，而是：

- 锁竞争统计异常
- wakeup 丢失
- starvation 增多
- 长链 handoff 崩掉

## 当前已知现象

当前 robust suite 中，复杂压力 workload 默认没有把 `mlfq` 列入必过集合。

原因不是实现缺失，而是：

- 当前教学版 `mlfq`
- 在“持续 handoff + 持续虚拟 tick + 混合压力”下
- 会表现出明显的病理性 starvation

这正说明 `t4l45` 作为 baseline 是有价值的：

- 它不只会报“通过”
- 它也能把策略弱点真实暴露出来

后续如果在真实中断或多核下继续保留 `mlfq`，应当把这个点当成重点观察对象。

## 关键文件

- [`src/main.rs`](./src/main.rs)
  trap 主循环、同步 syscall、trace/fault mode 接线
- [`src/processor.rs`](./src/processor.rs)
  调度器、调度统计、上下文切换记录、内核计数器
- [`src/process.rs`](./src/process.rs)
  `Process` / `Thread` / `SchedEntity`
- [`build.rs`](./build.rs)
  `t4l45` 用户程序打包
- [`test.sh`](./test.sh)
  统一测试入口
- [`scripts/run_suite.py`](./scripts/run_suite.py)
  宿主侧矩阵脚本
- [`../tg-rcore-tutorial-user/src/bin/t4l45_hybrid_pipeline.rs`](../tg-rcore-tutorial-user/src/bin/t4l45_hybrid_pipeline.rs)
  复杂混合压力测试
- [`../tg-rcore-tutorial-user/src/bin/t4l45_semaphore_ring.rs`](../tg-rcore-tutorial-user/src/bin/t4l45_semaphore_ring.rs)
  长链 handoff 压力测试

## 一句话总结

`t4l45` 当前已经是：

- 单核
- 线程级可插拔调度
- 带统一观测的同步实验平台
- 能做基础回归，也能做联合压力测试

它接下来要变成：

- 能被真实中断驱动
- 能扩展到多核
- 且仍然保持“可比较、可测量、可回归”的实验基线
