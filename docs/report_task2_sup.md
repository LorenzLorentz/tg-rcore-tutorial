## TASK2 补充报告：同步 bug 发现与汇报机制

### 1. 这份补充报告说明什么

这个文档实现 `t2l5` 里缺失的“同步互斥 bug 如何被发现、如何被统一汇报、如何被用户直观看到”的实现方案。

这次补充的目标不是重写整套同步框架，也不是额外做一套图形界面，而是在 **尽量少改现有内核结构** 的前提下，把原来“只能靠超时或 panic 猜测”的同步 bug，变成：

1. 有明确分类；
2. 有统一输出格式；
3. 有可以直接运行的用户态案例；
4. 有默认菜单入口，方便反复触发和观察。

现在的总体效果是：

1. 内核侧能直接发现一部分“精确型”同步 bug，例如死锁、非法解锁、线程带同步状态退出。
2. 内核侧也能发现一部分“启发式”同步 bug，例如 lost wakeup、所有线程都阻塞。
3. 用户态实验程序可以补上“统计型”或“语义误用型” bug，例如读者优先导致写者饥饿、`condvar` 用 `if` 而不是 `while`。
4. 这些 bug 最终都用统一前缀 `[t2l5-bug]` 输出到同一个控制台里，人眼和脚本都容易识别。

### 2. 如何给同步 bug 分类

我把这次要展示的 bug 分成三类。

#### 2.1 精确型 bug

这类 bug 的特点是：一旦条件成立，就可以直接认定“出错了”，不需要阈值，也不需要猜测。

当前覆盖的例子包括：

1. `mutex` 死锁；
2. `semaphore` 资源死锁；
3. 非法 `mutex_unlock`；
4. 线程退出时仍然持有锁或还在等待同步对象。

这类 bug 由内核直接报告，输出 `class=exact`。

#### 2.2 启发式 bug

这类 bug 的特点是：系统观察到了一种“非常不正常”的状态，虽然不一定能从程序语义上百分之百证明错误，但对教学实验来说已经足够说明“这里出问题了”。

当前覆盖的例子包括：

1. lost wakeup, 内核在 signal 或 unlock 时，如果发现等待队列里明明有人，但因为某种“注入故障”或逻辑错误，没有线程被移回 Ready 队列；
2. 所有线程都处于阻塞态，系统没有任何 ready task；
3. 自旋锁被某个线程长期占有，其他线程一直进不了临界区。

这类 bug 输出 `class=heuristic`。

#### 2.3 统计型 bug

这类 bug 更像是长期运行后暴露出来的异常分布，而不是某次单独操作就能直接判错。

当前覆盖的例子包括：

1. `reader-prefer rwlock` 导致 writer starvation。

这类 bug 由用户态实验程序基于统计阈值判断，输出 `class=statistical`。

### 3. 总体 pipeline

整体 pipeline 可以概括成：

同步操作发生
-> 更新等待关系 / 持有关系 / 条件变量等待状态 / 指标
-> 在合适时机做判定
-> 生成统一 bug event
-> 输出到控制台，并可被脚本或总结程序消费

### 4. 状态采集

#### 4.1 semaphore 的调试状态

在 [process.rs](/Users/lorenzlorentz/tg-rcore-tutorial/tg-rcore-tutorial-t2l5/src/process.rs#L56) 我增加了 `SemaphoreDeadlockState`，里面主要保存三类信息：

1. 每个信号量的总资源数 `totals`；
2. 每个线程当前持有了哪些信号量资源 `allocations`；
3. 每个线程当前正在等待哪个信号量 `waiting`。

有了这三个信息，内核在 `semaphore_down()` 之前就可以回答两个关键问题：

1. 这次请求是否会形成资源死锁；
2. 如果形成了死锁，当前系统里谁持有资源、谁在等待资源。

为了可视化汇报，我还加了 [describe_deadlock_snapshot()](/Users/lorenzlorentz/tg-rcore-tutorial/tg-rcore-tutorial-t2l5/src/process.rs#L169)，它会把当前请求、available 向量、waiting 关系和 allocations 一并串成一条快照文本。

#### 4.2 mutex 的调试状态

在 [process.rs](/Users/lorenzlorentz/tg-rcore-tutorial/tg-rcore-tutorial-t2l5/src/process.rs#L223) 我增加了 `MutexDeadlockState`，主要保存：

1. 每把 mutex 当前的 owner `owners`；
2. 每个线程当前正在等待哪个 mutex `waiting`。

这套数据结构足够构建一个轻量级的 wait-for graph：

1. `T -> M` 表示线程在等这把锁；
2. `M -> T` 表示这把锁当前归某个线程所有。

于是，一旦 `mutex_lock()` 前发现成环，就可以直接认定死锁成立。

为了让死锁“能被直观看到”，我又加了 [describe_wait_chain()](/Users/lorenzlorentz/tg-rcore-tutorial/tg-rcore-tutorial-t2l5/src/process.rs#L330)，输出类似：

```text
T3 -> M1 -> T2 -> M0 -> T3
```

这比单纯返回一个错误码直观得多。

#### 4.3 condvar 的等待状态

条件变量本身不太适合做通用“错误语义判定”，但它非常适合纳入“线程当前为什么阻塞”的总视图。

因此在 [process.rs](/Users/lorenzlorentz/tg-rcore-tutorial/tg-rcore-tutorial-t2l5/src/process.rs#L369) 我增加了 `CondvarWaitState`，并在 `Process` 中加入：

1. [condvar_waiting](/Users/lorenzlorentz/tg-rcore-tutorial/tg-rcore-tutorial-t2l5/src/process.rs#L415)；
2. [record_condvar_wait()](/Users/lorenzlorentz/tg-rcore-tutorial/tg-rcore-tutorial-t2l5/src/process.rs#L426)；
3. [clear_condvar_wait()](/Users/lorenzlorentz/tg-rcore-tutorial/tg-rcore-tutorial-t2l5/src/process.rs#L436)。

这样调度器在“系统全阻塞”时，就能把 `condvar` wait 也一起打印出来，而不只是知道“某线程 blocked 了”。

#### 4.4 统一的 blocked-thread 视图

在 [describe_blocked_threads()](/Users/lorenzlorentz/tg-rcore-tutorial/tg-rcore-tutorial-t2l5/src/process.rs#L440) 里，我把 mutex、semaphore、condvar 三类等待统一整理成文本。

它的价值在于：当调度器发现 `no task` 时，不需要分别去查多个子系统，而是可以直接拿到一份进程级 blocked 摘要。

#### 4.5 线程退出时的同步状态

在 [describe_thread_sync_state()](/Users/lorenzlorentz/tg-rcore-tutorial/tg-rcore-tutorial-t2l5/src/process.rs#L489) 里，我统一整理了线程的同步现场，包括：

1. 还持有哪些 mutex；
2. 还持有哪些 semaphore 资源；
3. 正在等哪个 mutex；
4. 正在等哪个 semaphore；
5. 是否还挂在 condvar 上。

这样线程退出时就可以判断：它是不是把同步对象“带进坟墓里”了。

### 5. 判定层

#### 5.1 mutex deadlock

在 [main.rs](/Users/lorenzlorentz/tg-rcore-tutorial/tg-rcore-tutorial-t2l5/src/main.rs#L1185) 的 `mutex_lock()` 中：

1. 先根据 `MutexDeadlockState` 检查 `would_deadlock()`；
2. 如果成环，直接输出 `class=exact kind=deadlock primitive=mutex`；
3. 再用 `describe_wait_chain()` 输出 `[t2l5-bug-graph]`；
4. 最后返回 `-0xdead` 给用户态程序。

这样用户态程序既能拿到返回值，也能在系统控制台上直接看到等待链。

#### 5.2 semaphore deadlock

在 [main.rs](/Users/lorenzlorentz/tg-rcore-tutorial/tg-rcore-tutorial-t2l5/src/main.rs#L1078) 的 `semaphore_down()` 中：

1. 先检查 `would_deadlock()`；
2. 如果成立，就用 `describe_deadlock_snapshot()` 生成一条资源分配快照；
3. 输出 `class=exact kind=deadlock primitive=semaphore`；
4. 返回 `-0xdead`。

mutex 用的是等待链，semaphore 用的是资源分配快照，两者都保持“人能直接读懂”。

#### 5.3 illegal unlock

在 [main.rs](/Users/lorenzlorentz/tg-rcore-tutorial/tg-rcore-tutorial-t2l5/src/main.rs#L1135) 的 `mutex_unlock()` 中：

1. 先查这把 mutex 的 owner；
2. 如果当前线程并不是 owner；
3. 直接输出 `class=exact kind=illegal_unlock primitive=mutex`；
4. 返回 `-1`。

这类错误最适合 fail-fast，因为继续运行只会污染状态。

#### 5.4 lost wakeup

为了让 lost wakeup 不再只表现成“程序永远不结束”，我利用现有故障注入点，在：

1. [semaphore_up()](/Users/lorenzlorentz/tg-rcore-tutorial/tg-rcore-tutorial-t2l5/src/main.rs#L1051)
2. [mutex_unlock()](/Users/lorenzlorentz/tg-rcore-tutorial/tg-rcore-tutorial-t2l5/src/main.rs#L1135)

里加了 fault-mode 分支：

1. 唤醒逻辑故意不把 waiter 重新入队；
2. 但同时立即输出 `class=heuristic kind=lost_wakeup`。

这样实验时不需要再靠几十秒 `timeout` 才知道“这里出 bug 了”。

#### 5.5 all threads blocked

在调度主循环里，如果 [find_next()](/Users/lorenzlorentz/tg-rcore-tutorial/tg-rcore-tutorial-t2l5/src/main.rs#L335) 返回空，我会进入 [report_scheduler_stall()](/Users/lorenzlorentz/tg-rcore-tutorial/tg-rcore-tutorial-t2l5/src/main.rs#L247)。

这个函数会：

1. 查询当前还有多少 active thread；
2. 枚举所有 active process；
3. 调用每个进程的 `describe_blocked_threads()`；
4. 如果所有线程都 blocked，就输出 `class=heuristic kind=all_threads_blocked primitive=system`。

这类输出特别适合教学，因为它把“系统卡死”直接翻译成“谁在等什么”。

#### 5.6 thread exit with sync state

在线程因 `EXIT` 或信号退出时，我会调用 [report_thread_exit_sync_bug()](/Users/lorenzlorentz/tg-rcore-tutorial/tg-rcore-tutorial-t2l5/src/main.rs#L226)。

它会检查 `describe_thread_sync_state()`，如果线程退出时还持锁、还持有信号量资源或还在等待同步对象，就输出：

```text
[t2l5-bug] source=kernel class=exact kind=thread_exit_with_sync_state primitive=system ...
```

这类 bug 以前往往只会在后续表现成“莫名其妙的卡住”，现在会在退出点直接被点出来。

### 6. 汇报层

#### 6.1 统一输出格式

我专门把内核输出统一成：

```text
[t2l5-bug] source=kernel class=exact|heuristic kind=... primitive=... ...
```

对应实现位于 [report_kernel_bug()](/Users/lorenzlorentz/tg-rcore-tutorial/tg-rcore-tutorial-t2l5/src/main.rs#L208)。

如果 bug 需要额外展示等待链或图结构，则额外输出：

```text
[t2l5-bug-graph] source=kernel kind=... primitive=... graph=...
```

对应实现位于 [report_kernel_bug_graph()](/Users/lorenzlorentz/tg-rcore-tutorial/tg-rcore-tutorial-t2l5/src/main.rs#L219)。

统一格式的好处是：

1. 人眼容易扫描；
2. 脚本容易匹配；
3. 内核和用户态都能共享同一前缀。

#### 6.2 用户态也用同一个前缀

在 [sync_lab.rs](/Users/lorenzlorentz/tg-rcore-tutorial/tg-rcore-tutorial-user/src/sync_lab.rs#L332) 我加入了 `print_user_bug()`：

```text
[t2l5-bug] source=user class=... kind=... primitive=... ...
```

这样虽然 bug 是在用户程序里判定的，但最终仍然出现在同一个 QEMU 控制台里，并且格式与内核保持一致。

### 7. 计数器与 summary

#### 7.1 内核 bug 计数器

在 [processor.rs](/Users/lorenzlorentz/tg-rcore-tutorial/tg-rcore-tutorial-t2l5/src/processor.rs#L67) 我扩展了 `KernelMetrics`：

1. `bug_total`
2. `bug_exact`
3. `bug_heuristic`
4. `bug_statistical`

目前内核真正自动累加的是：

1. `bug_total`
2. `bug_exact`
3. `bug_heuristic`

`bug_statistical` 字段保留了接口位置，但统计型 bug 目前仍主要由用户态案例直接输出 marker，而不是让内核替它计数。这样做是为了保持最小改动，不把“实验统计逻辑”强行推回内核。

#### 7.2 trace 读取接口

在 [main.rs](/Users/lorenzlorentz/tg-rcore-tutorial/tg-rcore-tutorial-t2l5/src/main.rs#L800) 的 `Trace` 实现里，我补了新的请求号：

1. `T2L5_TRACE_GET_BUG_TOTAL`
2. `T2L5_TRACE_GET_BUG_EXACT`
3. `T2L5_TRACE_GET_BUG_HEURISTIC`
4. `T2L5_TRACE_GET_BUG_STATISTICAL`

用户态则在 [sync_lab.rs](/Users/lorenzlorentz/tg-rcore-tutorial/tg-rcore-tutorial-user/src/sync_lab.rs#L320) 通过 `kernel_metrics()` 统一读取这些值。

#### 7.3 正确程序的 summary 也带 bug 计数

我把正常 demo 的 `[t2l5-summary]` 也统一扩展了 `bug_total / bug_exact / bug_heuristic / bug_statistical` 字段。

这样“正确程序”的输出不再只展示性能指标，也能顺手证明：

1. 这轮实验中有没有触发 bug；
2. 如果没有，bug 计数器应保持为 0。

### 8. 为什么统计型 bug 放在用户态

这里有一个很重要的设计取舍。

像 `rwlock starvation` 这样的 bug，本质上不是“某条 syscall 当下就错了”，而是“在一段时间窗口内，等待分布非常不合理”。

这类问题如果强行放进内核，会带来几个坏处：

1. 内核需要知道教学实验选择了什么阈值；
2. 内核需要知道哪个 workload 才算 starvation；
3. 内核需要长期保存实验相关统计，侵入性会变大。

所以我把它留在用户态：

1. `sync_lab.rs` 提供通用统计器 `AtomicStats`；
2. demo 程序自己设定阈值；
3. 一旦超过阈值，就调用 `print_user_bug("statistical", ...)`。

这样结构更清晰，也更符合“内核只做自己真正知道的事情”。

### 9. 用户态案例如何展示三类 bug

#### 9.1 精确型：`t2l5_bug_deadlock_mutex`

在 [t2l5_bug_deadlock_mutex.rs](/Users/lorenzlorentz/tg-rcore-tutorial/tg-rcore-tutorial-user/src/bin/t2l5_bug_deadlock_mutex.rs#L11) 中：

1. 启用死锁检测；
2. 创建一把阻塞 mutex；
3. 同一个线程连续 lock 两次。

第二次 lock 会触发内核 `mutex deadlock` 检测，控制台会出现：

```text
[t2l5-bug] source=kernel class=exact kind=deadlock primitive=mutex ...
[t2l5-bug-graph] source=kernel kind=deadlock primitive=mutex graph=T0 -> M0 -> T0
```

#### 9.2 启发式：`t2l5_spin_broken`

在 [t2l5_spin_broken.rs](/Users/lorenzlorentz/tg-rcore-tutorial/tg-rcore-tutorial-user/src/bin/t2l5_spin_broken.rs#L25) 中：

1. 主线程先拿住 spinlock；
2. 再创建 worker；
3. worker 长时间进不了临界区；
4. 程序自己输出：

```text
[t2l5-bug] source=user class=heuristic kind=stuck_spin primitive=spinlock ...
```

这里我专门让程序在报告后释放锁并收尾退出，避免它真的无限卡死，影响后续操作。

#### 9.3 精确型但由用户程序判定：`t2l5_condvar_if_bug`

在 [t2l5_condvar_if_bug.rs](/Users/lorenzlorentz/tg-rcore-tutorial/tg-rcore-tutorial-user/src/bin/t2l5_condvar_if_bug.rs#L46) 中：

1. 故意把 `condvar` 等待写成 `if` 而不是 `while`；
2. 两个 consumer 和一个 producer 并发运行；
3. 如果出现线程失败，就输出：

```text
[t2l5-bug] source=user class=exact kind=condvar_if_misuse primitive=condvar ...
```

这里之所以是 `source=user`，不是因为 bug 不重要，而是因为内核本身不知道用户的谓词逻辑，只能由案例程序自己判定。

#### 9.4 统计型：`t2l5_rwlock_reader_pref`

在 [t2l5_rwlock_reader_pref.rs](/Users/lorenzlorentz/tg-rcore-tutorial/tg-rcore-tutorial-user/src/bin/t2l5_rwlock_reader_pref.rs#L70) 中：

1. 创建读者优先读写锁；
2. 大量 reader 与少量 writer 并发；
3. 当 writer 等待时间超过 `WRITER_STARVATION_US` 阈值时，输出：

```text
[t2l5-bug] source=user class=statistical kind=starvation primitive=rwlock ...
```

### 10. 总结

这次补充实现的核心，不是“多写几种同步原语”，而是把同步 bug 从：

1. 只能靠 `timeout` 或 `panic` 猜测；
2. 只能靠人肉读日志反推；

变成：

1. 内核和用户态都能主动报告；
2. 精确型、启发式、统计型三类 bug 都有清晰归属；
3. 输出格式统一；
4. 默认 `cargo run` 就能直接进入实验菜单；
5. 运行案例和退出系统都具备完整闭环。

对我来说，这样的 `t2l5` 才真正像一个“同步 bug 实验平台”，而不只是若干零散的同步题目。
