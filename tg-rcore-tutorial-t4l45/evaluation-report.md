# T4L45 Final Evaluation Report

## 1. 评测对象

本次最终评测覆盖 3 个对象：

1. `baseline`
   - 目录：`tg-rcore-tutorial-t4l45-baseline`
   - 语义：恢复出的单核 baseline
   - 依赖：不带后缀的旧基础组件
2. `mp-smp1`
   - 目录：`tg-rcore-tutorial-t4l45-current-smp1-eval`
   - 语义：当前 MP 实现，但强制单核运行
   - 目的：隔离“实现本身的额外开销”和“多核收益”
3. `mp-smp2`
   - 目录：`tg-rcore-tutorial-t4l45-current-smp2-eval`
   - 语义：当前 MP 实现，双核运行
   - 目的：评估真实多核能力和稳定性

其中：

- `baseline` 明确依赖不带后缀的旧组件
- `mp-smp1/mp-smp2` 明确依赖 `-mp` 后缀的多核版基础组件

## 2. 评测环境

- 日期：`2026-04-04`
- 宿主系统：`Darwin 24.6.0 arm64`
- `qemu-system-riscv64`：`10.2.1`
- `cargo`：`1.93.1`
- `rustc`：`1.93.1`
- 构建方式：`cargo run --offline`

## 3. 方法学说明

### 3.1 为什么最终结果只采用串行评测

这次最终报告只采用串行评测结果，不采用更早的并行评测结果。

原因是三个评测对象的 `build.rs` 都会调用同一个 `../tg-rcore-tutorial-user`，而用户态程序默认共享同一个 `../tg-rcore-tutorial-user/target`。如果并行跑不同 target，不同的 `T4L45_SCENARIO/T4L45_SCHED` 会在共享用户构建目录里互相覆盖，导致：

- 宿主侧 `cargo run` 看起来成功
- guest 里实际运行的 workload 不是期望的那个 case

因此最终评测采用：

1. 单进程
2. 严格串行
3. 顺序固定为 `baseline -> mp-smp1 -> mp-smp2`

### 3.2 最终评测清单

| 套件 | 说明 | 每个 target 的用例数 |
|---|---|---:|
| `scheduler` | `cpu/io/interactive/mixed x fcfs/sjf/rr/mlfq/cfs` | 20 |
| `sync` | 成功用例套件 | 6 |
| `control` | 故障注入与负对照 | 5 |
| `robust_sync` | 同步原语跨调度器回归 | 15 |
| `robust_complex` | 综合复杂场景 | 8 |
| `mixed/rr` repeat | 重复 5 次，用于统计均值和波动 | 5 |
| `smp_probe` | 多核可见性探针 | 1 |
| `cpu/fcfs` repeat | 仅 `mp-smp2`，重复 10 次做稳定性回归 | 10 |

总运行次数：

- `baseline`：`60`
- `mp-smp1`：`60`
- `mp-smp2`：`70`
- 合计：`190`

### 3.3 测例构造与代表性

本次评测不是只挑一个 benchmark，而是按“调度性能、同步正确性、故障注入、复杂场景、多核可见性”五类目标来构造测例。

调度 workload：

- `cpu`
  - 5 个线程只做不同强度的 `busy_spin + lab_tick`
  - 用来突出 CPU-bound 负载下调度策略本身的差异
- `io`
  - 4 个 worker 阻塞在各自信号量上，1 个 producer 轮流释放它们
  - 用来模拟外部事件驱动的睡眠-唤醒型负载
- `interactive`
  - 4 个轻量 worker 高频等待，event loop 小粒度轮流唤醒
  - 用来放大交互型 workload 的尾延迟和响应时间差异
- `mixed`
  - 2 个 CPU 线程 + 3 个 interactive 线程 + 1 个 event loop
  - 用来逼近“后台计算 + 前台交互 + 同步唤醒”并存的真实系统场景

同步成功用例：

- `t2l5_spin_ticket`
  - 6 线程公平自旋锁竞争
- `t2l5_mutex_stress`
  - 6 线程 blocking mutex 保护共享计数器
- `t2l5_semaphore_pc`
  - `3 producer + 3 consumer + bounded buffer`
- `t2l5_condvar_pc`
  - `2 producer + 1 consumer + while-guarded predicate`
- `t2l5_rwlock_fair`
  - `5 reader + 2 writer`，要求 writer starvation 为 0
- `t2l5_phil_mutex`
  - 哲学家就餐资源竞争

负对照：

- `t2l5_spin_broken`
  - 主线程永不放锁，预期 `timeout`
- `mutex_drop_wakeup`
  - 内核故意丢 mutex 唤醒，预期 `timeout`
- `semaphore_drop_wakeup`
  - 内核故意丢 semaphore 唤醒，预期 `timeout`
- `t2l5_condvar_if_bug`
  - 用户态故意把 `while` 写成 `if`，预期命中 marker
- `t2l5_rwlock_reader_pref`
  - 故意使用读者优先读写锁，预期 writer 饥饿并命中 marker

复杂场景：

- `t4l45_hybrid_pipeline`
  - `2 producer + 2 consumer + 1 CPU hog`，共享小缓冲区，用 `mutex + condvar`
  - 用来观察调度、阻塞、唤醒和 CPU 干扰叠加后的表现
- `t4l45_semaphore_ring`
  - 6 线程信号量接力环
  - 用来放大 handoff 路径中的 blocked/wakeup/context-switch 稳定性问题

多核探针：

- `t4l45_smp_probe`
  - 8 线程重复采样 `current_hart_id()`
  - 直接回答“是否真的看到了 2 个 hart 共同工作”

这些测例组合起来，能把：

- 调度策略变化
- 同步语义正确性
- 复杂场景下的综合行为
- 真实多核可见性

分开观测，所以对 task4 这种“既改中断又改 SMP 又改同步”的任务来说，比单一 benchmark 更严谨。

## 4. 原始产物位置

- 结构化汇总：`docs/report_task4_eval_seq.json`
- 原始日志：
  - `tg-rcore-tutorial-t4l45-baseline/.logs/eval_seq/`
  - `tg-rcore-tutorial-t4l45-current-smp1-eval/.logs/eval_seq/`
  - `tg-rcore-tutorial-t4l45-current-smp2-eval/.logs/eval_seq/`

`docs/report_task4_eval_seq.json` 是最终权威结果；本报告只做人工解读。

## 5. 总体结果

| target | scheduler | sync | control | robust_sync | robust_complex | smp_probe |
|---|---:|---:|---:|---:|---:|---|
| `baseline` | `20/20` | `6/6` | `2/5` | `15/15` | `8/8` | 预期失败，只看到 `mask=0x1` |
| `mp-smp1` | `20/20` | `6/6` | `5/5` | `15/15` | `8/8` | 预期失败，只看到 `mask=0x1` |
| `mp-smp2` | `20/20` | `6/6` | `5/5` | `13/15` | `8/8` | 通过，`harts_seen=2 mask=0x3` |

### 5.1 baseline 的结论

- 恢复出的 baseline 已经足够作为单核正向基线：
  - 调度矩阵完整通过
  - 同步成功用例完整通过
  - 复杂综合场景完整通过
- 但 `control` 只有 `2/5` 和脚本预期一致：
  - `t2l5_spin_broken` 期望 `timeout`，实际 `ok`
  - `t2l5_mutex_stress + mutex_drop_wakeup` 期望 `timeout`，实际 `ok`
  - `t2l5_semaphore_pc + semaphore_drop_wakeup` 期望 `timeout`，实际 `ok`

解释上应保持克制：

- 这说明恢复出的 baseline 足够做正向性能和稳定性对比
- 但它不能被理解为“对旧负对照行为的字节级还原”

### 5.2 mp-smp1 的结论

- 所有单核正向套件都通过
- `control 5/5` 也与脚本预期一致
- 因为它保留了 MP 版 trap/timer/调度路径，所以非常适合作为“多核实现的单核对照组”

### 5.3 mp-smp2 的结论

- 多核调度矩阵 `20/20` 通过
- 基础同步成功套件 `6/6` 通过
- `control 5/5` 通过
- `robust_complex 8/8` 通过
- 仅剩 `robust_sync` 中 `t2l5_rwlock_fair` 在 `sjf/cfs` 两项失败

失败日志：

- `tg-rcore-tutorial-t4l45-current-smp2-eval/.logs/eval_seq/robust/t2l5_rwlock_fair__sjf.log`
- `tg-rcore-tutorial-t4l45-current-smp2-eval/.logs/eval_seq/robust/t2l5_rwlock_fair__cfs.log`

这两项都不是“卡死”，而是用户态断言失败，没有产出 `[t2l5-summary]`。`sjf` 的代表性报错是：

- `src/bin/t2l5_rwlock_fair.rs:105`
- `assertion left == right failed`
- `left: 1`
- `right: 0`

## 6. 多核正确性与稳定性

### 6.1 `smp_probe`

`smp_probe` 的目标不是“都应该 pass”，而是区分“真的有 2 个 hart 在工作”还是“只是换了多核内核代码但实际上仍是单核可见”。

结果如下：

| target | 结果 |
|---|---|
| `baseline` | panic：`expected at least 2 harts, saw mask=0x1` |
| `mp-smp1` | panic：`expected at least 2 harts, saw mask=0x1` |
| `mp-smp2` | `[t4l45-summary] case=smp_probe threads=8 samples=96 harts_seen=2 migrated_threads=8 mask=0x3` |

解释：

- `baseline` 和 `mp-smp1` 只看到一个 hart，符合预期
- `mp-smp2` 在用户态确实观测到了两个 hart，并且 8 个线程都发生过迁移

### 6.2 `cpu/fcfs` 连跑稳定性

在修复 portal 共享 `&mut` 风险之后，我专门对 `mp-smp2` 做了 `cpu/fcfs` 连跑 10 次回归，用来确认旧的“中途重启/重新打印 boot banner” blocker 是否还存在。

结果：

- `10/10` 成功
- 没再复现中途重启

统计如下：

| metric | mean | stdev | min | max |
|---|---:|---:|---:|---:|
| `avg_wait_us` | 338042.7 | 61653.6 | 307833 | 501321 |
| `avg_turnaround_us` | 1012945.9 | 40484.6 | 988737 | 1096173 |
| `throughput_milli_per_s` | 2409.3 | 11.8 | 2377 | 2423 |
| `starvation` | 6.4 | 0.7 | 6 | 8 |
| `ctx_switches` | 24.0 | 0.0 | 24 | 24 |

这说明旧 blocker 已经从“稳定复现的问题”变成“至少在这 10 次回归里不再复现”。

## 7. `mixed/rr` 五次重复性能比较

为了避免单次运行偶然性，我把 `mixed/rr` 在三个 target 上都重复跑了 5 次，并取均值。

| metric | baseline | mp-smp1 | mp-smp2 | baseline->mp-smp1 | mp-smp1->mp-smp2 | baseline->mp-smp2 |
|---|---:|---:|---:|---:|---:|---:|
| `avg_wait_us` | 264253.4 | 260587.2 | 138143.2 | `-1.4%` | `-47.0%` | `-47.7%` |
| `avg_turnaround_us` | 1236134.6 | 1169973.2 | 1254744.6 | `-5.4%` | `+7.2%` | `+1.5%` |
| `throughput_milli_per_s` | 2212.2 | 2326.8 | 2349.0 | `+5.2%` | `+1.0%` | `+6.2%` |
| `p95_latency_us` | 174.2 | 221.4 | 77.6 | `+27.1%` | `-65.0%` | `-55.5%` |
| `p99_latency_us` | 205.4 | 271.6 | 113.8 | `+32.2%` | `-58.1%` | `-44.6%` |
| `starvation` | 8.8 | 11.6 | 2.4 | `+31.8%` | `-79.3%` | `-72.7%` |
| `ctx_switches` | 97.0 | 97.0 | 97.0 | `+0.0%` | `+0.0%` | `+0.0%` |

### 7.1 解释

这张表给出的最准确结论是：

1. `mp-smp1` 不是 baseline 的无代价替代
   - 它在吞吐和平均周转上略优
   - 但尾延迟和 starvation 更差
2. `mp-smp2` 相比 `mp-smp1` 的改进很明确
   - 等待时间几乎减半
   - `p95/p99` 明显下降
   - starvation 明显下降
3. `mp-smp2` 相比 `baseline` 的优势主要体现在响应性
   - `avg_wait`、`p95`、`p99`、`starvation` 都显著改善
   - 吞吐也略有提升
   - 但平均周转时间没有同步下降

所以当前双核版本更准确的结论是：

- “响应性显著变好”

而不是：

- “所有指标都已经稳定全面领先”

## 8. 调度矩阵的结构性观察

完整 60 条调度结果已经全部写入 `docs/report_task4_eval_seq.json`。这里只总结几个稳定出现的结构性规律：

1. `baseline`
   - `cpu` 场景下 `sjf/cfs` 的平均等待最好
   - `mixed` 场景下 `cfs` 的平均等待最低，`sjf` 的平均周转与吞吐更强
2. `mp-smp1`
   - 单核下整体趋势与 baseline 接近
   - 但带中断和 MP 簿记后的机制成本已经开始显现
3. `mp-smp2`
   - `cpu` 和 `mixed` 下 `cfs` 的平均等待显著最好
   - `io/interactive` 场景下，双核把 `fcfs/rr` 的平均等待也压得很低
   - 说明多核收益和调度策略不是简单叠加，而是会改变 workload 的主导瓶颈

## 9. 当前剩余问题

最终评测里还明确留下 2 个需要保守表述的点：

1. `baseline` 的负对照行为不是完全恢复
   - 因此 baseline 更适合作为单核正向对比基线，而不是负对照行为基线
2. `mp-smp2` 的 `t2l5_rwlock_fair` 在 `sjf/cfs` 下仍然不稳定
   - 这不是系统级重启问题
   - 但它说明“双核 + 某些调度策略 + 公平读写锁”这条路径还有剩余并发语义问题

## 10. 最终结论

在最终串行、可复核的 190 次评测下，可以得出下面这组结论：

1. `t4l45` 的 baseline 已恢复到可用于正向评测的状态。
2. 当前 MP 实现已经完成真实 timer interrupt、多核启动与多核调度的主目标。
3. `mp-smp2` 在用户态已经能稳定观测到两个 hart 共同工作，并能看到线程迁移。
4. `mp-smp2` 在代表性的 `mixed/rr` 上，响应性相对 baseline 和 `mp-smp1` 都有明显提升。
5. 旧的 `cpu/fcfs` 重启 blocker 在当前代码状态下已不再复现。
6. 仍需继续修掉 `t2l5_rwlock_fair + sjf/cfs + smp2` 这条剩余边界，才能把 SMP 稳定性说到更满。
