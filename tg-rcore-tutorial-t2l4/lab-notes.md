# t2l4 实验过程记录

## 第 0 轮：先判断这章到底缺了什么

我先读了 `tg-rcore-tutorial-t2l4/README.md`、`exercise.md`、`src/main.rs`、`src/process.rs`、`src/processor.rs`。

我问 AI 的第一个问题不是“怎么直接做完”，而是：

> 这一章现在到底是“还没开始做”，还是“已经有一半框架，只差补算法”？

AI 帮我定位后，我确认了三件事：

1. 这个目录基本上就是 `ch8` 的直接拷贝。
2. 当前 ready queue 还是单一 FIFO，没有策略插件层。
3. 当前也没有统一 trace collector，更没有自动化 workload 对比脚本。

所以这次不能按“补几个函数”的方式做，而是得先把实验骨架搭起来。

## 第 1 轮：为什么不能硬等真实时钟中断

我原本第一反应是直接做 `on_tick()`，让 RR / MLFQ / CFS-like 按真实 timer interrupt 跑起来。

但继续读代码后，我发现这章基线还是 `ch8` 的 trap 模式：

- 有系统调用。
- 有阻塞 / 唤醒。
- 没有现成的时钟中断抢占链路。

这时 AI 提醒我先别急着把题目理想化，而是要问：

> 在现在这份代码里，什么东西是“已经有的”，什么东西是“你再多做一步就会把任务规模炸掉的”？

我最后接受的方案是：

- 真实阻塞 / 唤醒仍然沿用 `ch8`。
- 额外提供一个用户态 `lab_tick()`。
- 用户程序在 CPU burst 之间主动打虚拟 tick。
- 内核把这个 tick 接到 `Scheduler::on_tick()`。

这样做的好处是：

- 不用重建整条 timer interrupt 路径；
- 仍然能在 QEMU 中比较五类策略；
- README 里可以明确解释这个简化假设。

## 第 2 轮：状态放在线程里，而不是进程里

这一轮我重点确认“调度实体到底应该挂在哪”。

我一开始还有点犹豫：是不是把统计状态集中放到 `Processor` 里更简单？

但读完 `ch8` 的线程模型后，结论很明确：

- 阻塞发生在线程层。
- 唤醒发生在线程层。
- 交互延迟也是“某线程被唤醒后多久再次运行”。

所以我把 `SchedEntity` 放进了 `Thread`：

- `ready_since_ns`
- `total_wait_ns`
- `burst_estimate_ns`
- `vruntime_ns`
- `queue_level`
- `interaction_latencies_ns`

AI 在这里给我的提醒很实用：

> 如果一个指标天然描述的是线程行为，就不要为了“看起来集中管理”把它硬抬到进程层。

## 第 3 轮：先改内核骨架，再写 workload

我这次没有先写 workload，因为如果内核还是原来的 `PThreadManager + FIFO`，
那用户态 workload 再漂亮，也只是把旧调度器跑很多遍。

所以当前阶段我先做了三件事：

1. 把 `processor.rs` 改成本地自管的 `ProcessorInner`。
2. 在里面加入策略枚举、trace collector、线程级统计。
3. 在 `main.rs` 里把 `trace` syscall 接成虚拟 tick。

做到这一步以后，后面的用户态 workload 才真的是“驱动实验”，而不是“驱动旧逻辑”。

## 第 4 轮：第一次跑起来后，我误以为是 workload 太重

我第一次把 `cpu` workload 跑进 QEMU 时，只看到了：

- 场景启动日志
- 没有 summary

我当时第一反应是：

> 是不是我把 busy loop 设太大了，QEMU 只是还没跑完？

所以我先做了两件保守的事：

1. 把 CPU / IO / interactive / mixed 的自旋参数整体缩小。
2. 给每个 worker 结束前补一条很短的 `done` 日志。

结果第二次运行时，5 个 worker 都明确打印了 `done`，但 summary 还是没出来。

这一步很关键，因为它让我把问题从“workload 太慢”收缩成了“收尾路径坏了”。

## 第 5 轮：真正卡住的不是调度，而是报告输出

为了继续缩范围，我又让 AI 帮我想一个最小诊断法：

> 如果最后一个线程已经退出，但内核没有结束，最短路径的定位点应该打在哪？

最后我加了两种只用于调试的信息：

- 线程退出时打印剩余线程数
- `find_next()` 返回空 ready queue 时打印一行

结果我看到：

- 最后一个线程退出了
- ready queue 已经空了
- 内核也确实进入了 `before report`

这说明调度和清理都已经走完，卡点只剩 `print_report()`。

根因比我预期更“工程化”：

- 我一开始在 summary 里直接用了浮点格式化；
- 在这条 `no_std + console write syscall` 输出链上，这样做非常不划算；
- 实际效果就是内核在报告格式化阶段卡得像死机一样。

最后我的修正方案是：

- 内核只输出整数口径：
  - `avg_wait_us`
  - `avg_turnaround_us`
  - `throughput_milli_per_s`
  - `p95_latency_us`
  - `p99_latency_us`
- 宿主侧 `run_suite.py` 再把这些整数换算成人看的毫秒 / 每秒任务数。

这个 bug 对我挺有提醒意义：内核里“看起来只是打印得更漂亮一点”的代码，实际上也可能变成收尾阶段的系统性卡点。

## 第 6 轮：验证思路从“能跑一次”改成“矩阵能批量跑”

如果这章最后还是靠我手动改环境变量、手动抄 20 次 `cargo run`，那其实还算不上实验套件。

所以我最后补了宿主侧脚本：

- `verify.sh`
- `scripts/run_suite.py`

我和 AI 一起确认的目标不是“再包一层 shell”，而是：

1. 自动进入 `~/rcore_docker.sh` 环境
2. 自动跑 `5 scheduler x 4 scenario`
3. 自动抓 `t2l4-summary`
4. 自动在终端打印表格
5. 自动把每次原始输出落到 `.logs/suite/`

这样这章才真正从“做了几个调度算法”变成“可批量比较的实验平台”。

## 第 7 轮：还有一个不是 t2l4 本体、但会直接拦路的兼容性问题

在把 userland workload 单独拿出来 `cargo check` 时，我还碰到了一个和本章逻辑无关、但不修就没法继续的问题：

- `tg-rcore-tutorial-syscall(user)` 里的 `asm!("ecall")`
- 在 Rust 2024 下触发了 `unsafe_op_in_unsafe_fn`
- 而这个 crate 又是 `#![deny(warnings)]`

所以它会直接把用户态 workload 的编译卡死。

这里我没有把问题绕过去，而是顺手把那几处 `asm!` 全部补成显式 `unsafe { ... }`。

我把这件事也记下来，是因为它很符合我这次和 AI 协作时形成的一个习惯：

> 真正拦路的东西，就算它不属于“题目核心”，也要尽快清掉，不然所有后续验证都会失真。

## 第 8 轮：完整矩阵跑完之后，我最关心的不是“哪种最好”，而是“差异有没有被同一口径测出来”

最后我通过 Docker + QEMU 跑完了完整矩阵：

- 5 个 scheduler
- 4 个 scenario

我没有把注意力只放在“谁第一名”，而是先看三件事：

1. 每一组都能稳定跑完并产出 summary。
2. 不同 workload 下指标确实出现了结构性差异。
3. 这些差异是同一套 collector / 同一套 summary 口径打出来的。

从这个角度看，这次 t2l4 我认为算是完成了。因为我交付的不是“几段调度代码”，而是一套：

- 可以切 scheduler
- 可以切 workload
- 可以统一采样
- 可以批量出表

的实验平台。
