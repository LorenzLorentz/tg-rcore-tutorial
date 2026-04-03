# t4l45 与 Baseline 的对比

## 1. 结论先行

这次交付不是在 baseline 上“小修补”，而是把它从：

- `lab_tick()` 驱动的单核调度实验骨架

推进成了：

- 支持真实 `SupervisorTimer` 中断
- 支持多 hart 启动、并行调度与 per-hart current
- 支持 SMP 下可工作的同步原语与内核堆分配

## 2. 与 baseline 的核心差异

| 维度 | baseline | 当前实现 |
|---|---|---|
| 时钟推进 | 依赖用户态 `lab_tick()` 主动打点 | 真实 timer interrupt，`SupervisorTimer` 进入调度路径 |
| 处理器模型 | 单个全局 `current`，默认单 hart | `MAX_HARTS` + per-hart current slot + `-smp 2` |
| 启动链路 | 只需 hart0 语义 | M 态按 hart 传入 `mhartid`，secondary hart 等待 boot stage 后进入 Rust |
| timer 编程 | 只覆盖单 hart `mtimecmp` | 按 `mhartid` 给每个 hart 编程各自的 `mtimecmp` |
| 地址空间 | 只保证启动 hart 可运行 | 所有 hart 进入调度前都显式激活内核页表 |
| 同步原语内部保护 | `UPIntrFreeCell/RefCell`，本质单核假设 | `spin::Mutex` 保护的 SMP-safe 队列/状态 |
| 内核堆分配 | 原 `tg-kernel-alloc` 按单处理器假设实现 | 本地 patch 后用 `spin::Mutex` 串行化 buddy allocator |
| 多核可观测性 | 用户态无法知道自己跑在哪个 hart | 新增 `T4L45_TRACE_GET_CURRENT_HART` + `t4l45_smp_probe` |
| SMP 竞态处理 | 不涉及 | 补了 `pending_wakeups`，接住“先唤醒、后落表”的 lost wakeup |

## 3. 这次新增/重构的关键点

### 3.1 中断响应

- 在主调度循环里显式处理 `scause::Interrupt::SupervisorTimer`
- 每次返回用户态前重新编程下一次 timer
- `lab_tick()` 仍保留为兼容性的轻量 trace 请求，但不再是唯一调度驱动

### 3.2 多核启动与调度

- SBI 启动入口按 hart 切分 M 模式栈
- secondary hart 自旋等待 `BOOT_STAGE == 1` 后进入 Rust
- `rust_main(hart_id)` 为每个 hart 建立自身的 `tp/sscratch`
- `Processor` 改成：
  - 全局可并发访问的 `ProcessorInner`
  - per-hart current slot
  - `running_tasks` 计数与多 hart 取任务/退休路径

### 3.3 SMP 下同步与内存分配

- `tg-sync` 的 mutex / semaphore / condvar 内部状态改为 `spin::Mutex`
- `ProcessorInner` 新增 `pending_wakeups`
  - 解决阻塞线程尚未写回 `threads` 表时，另一 hart 提前唤醒导致的丢失唤醒
- `.cargo/config.toml` 新增对本地 `tg-rcore-tutorial-kernel-alloc` 的 patch
- `tg-rcore-tutorial-kernel-alloc` 改为自旋锁保护的全局 buddy allocator

## 4. 验证结果

### 4.1 新增的多核探针

场景：`T4L45_SCENARIO=t4l45_smp_probe`

结果：

```text
[t4l45-summary] case=smp_probe threads=8 samples=96 harts_seen=2 migrated_threads=8 mask=0x3
[t4l45-sched-summary] scheduler=rr scenario=t4l45_smp_probe tasks=9 avg_wait_us=2684183 avg_turnaround_us=3803513 throughput_milli_per_s=1324 p95_latency_us=0 p99_latency_us=0 starvation=994 ctx_switches=1286
```

解释：

- `harts_seen=2` 直接说明用户态线程确实在两个 hart 上运行
- `mask=0x3` 表示 hart0/hart1 都被观测到
- `migrated_threads=8` 说明线程不仅并行运行，还发生了跨 hart 迁移

### 4.2 混合工作负载

场景：`T4L45_SCENARIO=mixed`

结果：

```text
[t4l45-sched-summary] scheduler=rr scenario=mixed tasks=7 avg_wait_us=137752 avg_turnaround_us=1477466 throughput_milli_per_s=1835 p95_latency_us=93 p99_latency_us=170 starvation=2 ctx_switches=97
```

解释：

- 这个场景同时覆盖 CPU burst、交互式线程、信号量唤醒与 producer 路径
- 说明真实时钟中断、多核调度和基础同步链路已经能在同一 workload 下稳定协作

### 4.3 同一实现下的 1 核 / 2 核数值对比

为了避免把不同代码版本混在一起，我额外做了一组“同一实现、只改 QEMU `-smp` 参数”的对比。

场景：`T4L45_SCENARIO=mixed`

| 指标 | `-smp 1` | `-smp 2` | 变化 |
|---|---:|---:|---:|
| `avg_wait_us` | 260283 | 137752 | `-47.1%` |
| `avg_turnaround_us` | 1214855 | 1477466 | `+21.6%` |
| `throughput_milli_per_s` | 2270 | 1835 | `-19.2%` |
| `p95_latency_us` | 264 | 93 | `-64.8%` |
| `p99_latency_us` | 303 | 170 | `-43.9%` |
| `starvation` | 12 | 2 | `-83.3%` |
| `ctx_switches` | 97 | 97 | 持平 |

这组数据说明：

- 双核最明显改善的是等待时间和尾延迟；
- 这一次样本里吞吐和平均周转时间没有同步变好，说明当前实现还没有到“稳定获得线性加速”的程度；
- 所以更准确的说法是：当前多核版本已经明显改善了交互/等待侧指标，但吞吐收益还需要更多轮次和更稳定的压力场景来确认。

### 4.4 扩展压力场景

我额外拿下面两类场景做过 bring-up 压测：

- `t4l45_semaphore_ring`
- `t4l45_hybrid_pipeline`

它们的价值主要在于暴露了两类 baseline 完全看不到的问题：

1. SMP 下的 lost wakeup
2. 单核版内核分配器在多 hart 并发分配时的假性 OOM

这两类问题已经分别通过 `pending_wakeups` 和 SMP-safe `kernel-alloc` 修掉。调试构建下这两个长时压力场景的收敛速度仍明显慢于基础场景，所以这次报告把它们作为“找 bug 的综合压力测试”，而不是唯一的交付 pass gate。

## 5. 仍需注意的点

- 这次交付已经把 baseline 的“单核教学实验骨架”推进到了“能真实响应中断、能在双 hart 上调度”的状态。
- 但如果后续要把 `semaphore_ring/hybrid_pipeline` 也纳入严格 CI pass gate，最好再补一轮：
  - 更长时间窗口下的自动化基准
  - `Processor` 运行态簿记的进一步精简
  - 更细粒度的长期压力测试数据收集
