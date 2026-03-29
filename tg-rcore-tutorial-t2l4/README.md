# 第九章：调度算法实验套件

本章不是在 `ch5` 上直接做小修小补，而是**基于 `tg-rcore-tutorial-ch8` 继续开发**，把“系统里只有一个固定 scheduler”升级成“可插拔 scheduler + 统一观测 + 可比较 workload”的实验平台。

这样选基线有两个原因：

- `ch8` 已经具备线程、阻塞、唤醒、同步原语等机制，天然支持 `on_block()` / `on_wakeup()` 这类调度器事件。
- 你要比较的不只是 CPU-bound 进程，还包括 IO-bound、交互型和混合 workload；这些场景在 `ch8` 的线程化模型里更容易构造和解释。

换句话说，`ch5` 是“第一次引入调度”的教学起点，而 `tg-rcore-tutorial-t2l4` 是“把调度做成实验平台”的工程起点。

## 练习任务（以教代学，学以致用）

- 学：理解 FCFS / SJF / RR / MLFQ / CFS-like（简化）等调度策略的目标、适用场景与局限。
- 教：把原本耦合在 `processor.rs` 中的单一调度逻辑抽象成统一接口，并补齐统一的数据采集链路。
- 用：基于用户态 workload 做自动化实验，比较不同策略在等待时间、周转时间、吞吐量、交互延迟和饥饿风险上的差异。

> 注：本目录当前只是从 `ch8` 复制出来的继续开发脚手架，内核代码主体仍是 `ch8` 基线；本 README 描述的是后续应实现的目标与设计。

## 参考资料

- 调度与实验平台设计参考：
  - <https://github.com/LearningOS/bachelor-thesis/blob/main/jyt-thesis.pdf>
  - <https://github.com/LearningOS/bachelor-thesis/blob/main/msy-thesis.pdf>

## 你要实现的核心内容

### 1. 调度器改为“策略插件”

建议统一成如下风格的调度器接口：

```rust
trait Scheduler {
    fn enqueue(&mut self, task: Tid);
    fn pick_next(&mut self) -> Option<Tid>;
    fn on_tick(&mut self, current: Tid);
    fn on_block(&mut self, current: Tid);
    fn on_wakeup(&mut self, task: Tid);
}
```

这里有两个设计决定值得提前定下来：

- **调度粒度选 Thread，而不是 Process。**
  原因是 `ch8` 已经把执行单元拆成线程，阻塞和唤醒也发生在线程层；如果继续按进程调度，会把很多实验现象人为抹平。
- **策略选择放在调度器内部，而不是散落在 Trap/syscall 逻辑中。**
  `main.rs` 负责触发事件，`processor.rs` 负责策略调度，`process.rs` 只保存调度实体所需的状态。

### 2. 统一数据采集

每次上下文切换都应记录统一事件，至少包括：

- 时间戳
- 当前就绪队列长度
- 被切出的任务
- 被切入的任务
- 本次运行片段时长
- 事件类型：`tick` / `block` / `wakeup` / `yield` / `exit`

建议把它抽象成单独的 `SchedTrace` / `SchedEvent`，不要把统计代码直接揉进某一个具体策略里。这样后续比较 FCFS、RR、MLFQ、CFS-like 时，使用的是同一套观测口径。

### 3. 统一观测指标

建议至少输出下面这些指标：

- 平均等待时间
- 平均周转时间
- 吞吐量
- P95 / P99 交互延迟
- 饥饿发生次数

建议在 README 或代码注释里明确这些指标的定义：

- 等待时间：任务处于 ready 但未运行的累计时间
- 周转时间：完成时间减到达时间
- 吞吐量：单位实验窗口内完成的任务数
- 交互延迟：任务被唤醒到第一次重新获得 CPU 的时间
- 饥饿：某任务等待时间超过阈值，例如 `N * base_quantum`

### 4. 自动化 workload 与验收

验收不应该只靠“看起来能运行”，而要用用户态 workload 驱动实验：

- CPU-bound：长计算、极少主动阻塞
- IO-bound：频繁 sleep / wait / block / wakeup
- 交互型：短 burst、频繁唤醒，重点测尾延迟
- 混合型：把上面三类组合在同一实验窗口中

验收输出至少要能在终端打印成表格；如果后续愿意扩展，也可以导出 CSV 再画图。

### 5. 可扩展点

本章建议保留下面这些继续演化的接口和配置项：

- MLFQ 的提升 / 下降策略
- 时间片动态调整
- 负载变化下的稳定性比较
- 是否开启 aging
- 是否开启 per-task burst prediction

## 我的设计建议

### 1. 在 `ch8` 的线程模型上做调度，而不是退回进程模型

这会直接影响你的数据解释方式。比如交互延迟、唤醒后等待时间、阻塞恢复时间，这些都更适合在线程层记录。进程可以作为聚合视角，但不应该是底层调度实体。

### 2. 每种策略只做“队列管理 + 选下一个任务”

不要把统计、日志和策略状态混在一起。一个更稳妥的组织方式是：

- `Scheduler`：只决定任务的入队、出队和时间片反应
- `SchedEntity`：保存在 `Thread` 或 `Process` 中的策略相关状态，例如 `vruntime`、`queue_level`、`burst_estimate`
- `SchedTraceCollector`：记录事件并在实验结束后计算指标

### 3. SJF 不要硬编码“真实 burst”，应使用可解释的估计值

教学环境里最合适的简化方式是指数平均：

```text
pred_next = alpha * actual_last + (1 - alpha) * pred_prev
```

这样它既保留了 SJF 的思想，又不会偷偷使用“未来信息”。

### 4. CFS-like 不需要照搬 Linux

这个实验的目标是比较思想，而不是复刻 Linux CFS。一个可接受的简化方案是：

- 维护 `vruntime`
- 权重来自 `priority` 或 `nice`
- 总是选择 `vruntime` 最小的线程
- 用最小堆或有序向量代替红黑树

只要你在 README 中把“简化了什么、为什么这样简化”说清楚，就足够了。

### 5. MLFQ 应该做成“参数化实验”，而不是写死常数

建议把下面这些参数都集中管理：

- 队列层数
- 每层时间片
- 降级条件
- 周期性全局提升间隔
- 交互任务优待策略

只有参数可调，后面才谈得上“实验套件”。

## 建议落点

<a id="source-nav"></a>

## 源码阅读导航索引

[返回根文档扩展章节导航](../README.md#extended-chapters-nav)

建议按“当前调度路径 -> 状态承载 -> 观测链路”阅读：

| 阅读顺序 | 文件 | 重点问题 |
|---|---|---|
| 1 | `src/processor.rs` | 现有 ready queue 是怎么工作的？调度点在哪里？ |
| 2 | `src/process.rs` | 线程/进程结构中，哪些字段适合承载调度策略状态？ |
| 3 | `src/main.rs` | `tick`、阻塞、唤醒、退出分别在哪些路径触发？ |
| 4 | `../tg-rcore-tutorial-task-manage` | 能否把可复用的队列/实体管理抽出去，而不是塞回章节代码？ |

## 推荐改动范围

| 路径 | 建议角色 |
|---|---|
| `tg-rcore-tutorial-t2l4/src/processor.rs` | 放调度器 trait、策略选择、核心 ready-queue 逻辑 |
| `tg-rcore-tutorial-t2l4/src/process.rs` | 放调度实体字段，例如 `queue_level`、`vruntime`、`burst_estimate` |
| `tg-rcore-tutorial-t2l4/src/main.rs` | 放事件钩子接入点，不放复杂策略细节 |
| `tg-rcore-tutorial-t2l4/exercise.md` | 放阶段性任务要求与验收口径 |
| `tg-rcore-tutorial-user` | 后续补 workload 程序，但本次不要求在这里给出测试实现 |

## 建议里程碑

1. 先把现有调度器抽成统一 trait，并保留一个“兼容 ch8 现状”的默认策略。
2. 再补 trace collector，保证任何策略都能输出同一格式的数据。
3. 然后实现 FCFS / RR 作为最小闭环。
4. 再实现 SJF / MLFQ / CFS-like，并开始做参数比较。
5. 最后再补 workload 套件和终端报告脚本。

## DoD 验收标准

- [ ] 能说明为什么本章基于 `ch8` 而不是 `ch5`
- [ ] 能给出统一调度器接口，并说明每个钩子的语义
- [ ] 能为至少 5 种策略复用同一套 trace collector
- [ ] 能解释等待时间、周转时间、吞吐量、交互延迟、饥饿统计的口径
- [ ] 能设计 CPU-bound / IO-bound / 交互型 / 混合型 workload
- [ ] 能输出终端可读的比较表

## 当前脚手架说明

本目录目前继承了 `ch8` 的代码、`build.rs` 和测试脚本基线，因此：

- 当前 `cargo run` 的实际行为仍接近 `ch8`
- 当前 `build.rs` 仍按 `ch8` 的用户程序集组织
- 当前 `test.sh` 仍是 `ch8` 风格测试脚本

这不是疏漏，而是刻意保留一个“能继续开发的起点”。你后续真正实现本章时，应优先修改 `README.md` 中列出的设计落点，再按实验目标重写 workload、测试脚本和数据导出流程。
