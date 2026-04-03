# t4l45 实现过程记录

## 第 0 轮：先把 baseline 真正看清楚

我先读了这些文件：

- `tg-rcore-tutorial-t4l45/README.md`
- `tg-rcore-tutorial-t4l45/src/main.rs`
- `tg-rcore-tutorial-t4l45/src/processor.rs`
- `tg-rcore-tutorial-t4l45/src/process.rs`
- `tg-rcore-tutorial-sync/src/{lib,up,mutex,semaphore,condvar}.rs`
- `tg-rcore-tutorial-sbi/src/{lib,msbi,m_entry.asm}`
- `tg-rcore-tutorial-kernel-context/src/{lib,foreign/mod.rs}`

先确认了 4 件关键事实：

1. `t4l45` 当前只有 `UserEnvCall` trap 主路径，还没有处理中断 trap。
2. 调度时钟推进仍然依赖用户态 `lab_tick()`，`handle_tick()` 只会在 `TRACE` syscall 里被调用。
3. `ProcessorInner` 只有一个全局 `current`，这意味着它的执行模型还是单 hart。
4. `tg-sync` 的保护模型仍然是“单核 + 关中断”，`UPIntrFreeCell`/`RefCell` 明确不适用于 SMP。

所以这次任务不是“补几个分支”，而是至少要同时改三层：

- trap/timer 链路
- 调度器与处理器状态
- 同步库的并发保护

## 当前已经识别出的难点

### 难点 1：`-bios none` 下的 timer 目前只写了单 hart 的 `mtimecmp`

`tg-sbi/src/msbi.rs` 现在把 timer 目标时间直接写到固定地址：

- `0x200_4000`

这对单 hart 可以工作，但一旦开多核，就需要确认：

- 每个 hart 对应的 `mtimecmp` 偏移
- 哪个 hart 负责重新编程 timer
- secondary hart 是否也需要独立 tick

这部分如果不先理清，多核启动后很容易出现“只有 hart0 有时钟”的假 SMP。

### 难点 2：当前调度主循环是单线程式的

`rust_main()` 里现在是：

- 取一个 `task`
- `execute()`
- 读一次 `scause`
- 处理 syscall / 退出 / 阻塞

这条路径默认假设：

- 当前只有一个 hart 会进入调度循环
- `PROCESSOR.get_mut()` 拿到的是唯一执行者

如果直接让多个 hart 同时进这段代码，当前写法会立刻数据竞争。

### 难点 3：`tg-sync` 现在的“临界区保护”在 SMP 下会失效

我确认 `tg-sync` 现在大量使用：

- `UPIntrFreeCell`
- `RefCell`
- 全局 `INTR_MASKING_INFO`

这意味着它只保证“当前核不被本核中断打断”，并不保证：

- 另一个 hart 不会同时进入相同数据结构

所以多核调度能不能稳定，不只看 `Processor`，还取决于等待队列、mutex/semaphore/condvar 内部状态是否改成真正的并发安全结构。

## 当前决定的推进顺序

我先按下面顺序做，而不是同时硬上：

1. 先把真实 timer interrupt 接进单核。
2. 再把 `Processor` 改成 per-hart current + 并发安全 ready 结构。
3. 然后把 `tg-sync` 从 UP-only 保护改成 SMP-safe。
4. 最后扩测试、跑验证、写 baseline 对比报告。

这样做的原因很直接：

- 如果单核真实中断都没站稳，多核只会把问题放大。
- 如果同步库不先补强，多核调度哪怕能跑，也很容易在高压测试里随机坏。

## 第 1 轮：先补一条“能证明多 hart 真的在干活”的观测链路

在读现有代码时，我发现虽然内核里已经预留了：

- `T4L45_TRACE_GET_CURRENT_HART = 0x204`

但这条 trace 还没有真正接到：

- `SyscallContext::trace()`
- `user_lib::sync_lab`
- 用户态测试用例

这会直接带来一个问题：即使 `-smp 2` 已经能跑，我也没法在用户态程序里验证“当前线程到底落在哪个 hart 上”。所以这一步我决定先补齐：

1. 内核 trace 分支，返回当前 hart 编号。
2. 用户态包装函数 `current_hart_id()`。
3. 新增 `t4l45_smp_probe`，用多线程采样 hart 位图，直接检查是否至少看到了两个 hart。

顺手还要删掉启动阶段为调试留下的 hart 字符输出，避免污染测试输出。

## 第 2 轮：多核 bring-up 真正卡住的不是调度算法，而是底层簿记

把 secondary hart 真正放进调度循环后，我连续撞到了几类不是 README 里能直接告诉我的问题：

### 2.1 当前 hart 的 Rust 可见标识不稳定

一开始我试图继续依赖 `tp`，但运行中发现它并不适合作为“随时可读的 hart id 来源”。最后改成：

- M 态把 `mhartid` 放进 `_start` 的 `a0`
- `rust_main(hart_id)` 启动后同时写 `tp` 和 `sscratch`
- 内核运行期统一从 `sscratch` 读当前 hart

这样 secondary hart 的 Rust 路径才能稳定拿到正确 hart 编号。

### 2.2 secondary hart 没有自己的内核页表

只有 hart0 建好了 `satp` 还不够；secondary hart 如果直接进调度，会在后续访问里踩空。所以我补了所有 hart 进入调度前都执行：

- `activate_kernel_space()`
- `sfence_vma_all()`

### 2.3 per-hart current 槽位会出现“旧镜像没退休”

我在 `install_current()` 上撞到了：

- `assertion failed: slot.current.is_none()`

这说明某些路径已经把线程状态写回了调度器，但 per-hart `current` 镜像还没清掉。这个镜像本质上只是运行态簿记，不是线程状态真源，所以我把这里改成：

- 若发现旧镜像，先回收旧 `running_tasks` 计数
- 再安装新的 current 记录

这样调试构建不会因为簿记残留直接 panic。

## 第 3 轮：SMP 下最隐蔽的问题是“唤醒先到，阻塞后落表”

扩展同步场景时，我发现单核思路下成立的时序在多核下不再成立：

1. 线程 A 在 `semaphore_down / mutex_lock / condvar_wait` 中已经判定“该阻塞”。
2. 但它还没来得及把自己写回 `ProcessorInner.threads` 这个 blocked 表。
3. 线程 B 在另一个 hart 上已经先执行了 `up / unlock / signal`，并尝试 `re_enque(A)`。
4. 这时 `threads` 表里还找不到 A，那次唤醒就被静默丢掉。

这就是典型的 lost wakeup，只在 SMP 下才真正暴露。

我最后补了一个 `pending_wakeups: BTreeSet<ThreadId>`：

- `re_enque()` 找不到目标线程时，先把这次唤醒记下来
- `make_running_blocked()` 把线程落回 blocked 表后，若发现自己已经被提前唤醒，就立刻转 ready

这个改动对 `semaphore_down / mutex_lock / condvar_wait` 都生效。

## 第 4 轮：真正把复杂场景打坏的是全局内核分配器还是单核版

进一步跑 `t4l45_semaphore_ring` 时，我抓到了一个更底层的事实：

- `tg-rcore-tutorial-kernel-alloc` 原实现明确按“单处理器 / 无并发访问”写的
- 多个 hart 同时做 `VecDeque/BTreeMap` 的分配和释放时，会把 buddy 元数据打坏
- 表面症状就是随机的 `memory allocation of 384 bytes failed`

这里还有一个很隐蔽的接线问题：

- 我虽然改了本地 `tg-rcore-tutorial-kernel-alloc`
- 但 `.cargo/config.toml` 一开始并没有 patch 这个 crate
- 所以前面的修复其实根本没进最终镜像

最后我做了两步：

1. 给 `tg-rcore-tutorial-kernel-alloc` 加 `spin::Mutex`，把全局 buddy allocator 串行化。
2. 在根目录 `.cargo/config.toml` 里显式 patch `tg-rcore-tutorial-kernel-alloc` 到本地路径。

补完以后，复杂同步场景里那类“假性 OOM”就消失了。

## 第 5 轮：扩展测试给我的结论

我没有只停留在 README 里的基础场景，而是额外用了这些场景做验证：

- `t4l45_smp_probe`：专门确认用户态确实观测到两个 hart 在执行
- `sched_lab_mixed`：覆盖 CPU + 同步阻塞 + producer/consumer 混合路径
- `t4l45_semaphore_ring` / `t4l45_hybrid_pipeline`：作为更重的综合同步压力场景

当前我确认的结论是：

1. `smp_probe` 能稳定证明“双 hart 都在参与执行”，不是假 SMP。
2. `mixed` 能稳定跑通，说明真实 timer interrupt + 多核调度 + 基础同步阻塞链路已经打通。
3. `semaphore_ring`/`hybrid_pipeline` 在 bring-up 期间确实帮我抓出了 lost wakeup 和 allocator 两类关键问题。
4. 这两类长时压力场景在调试构建下收敛速度明显慢于基础场景，所以我把它们更多作为“找 bug 的强压测试”，而不是这次交付里唯一的 pass gate。
