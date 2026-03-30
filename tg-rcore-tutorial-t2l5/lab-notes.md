# t2l5 实验过程记录

## 第 0 轮：先把“题目”翻译成“代码边界”

我先读了：

- `tg-rcore-tutorial-t2l5/README.md`
- `tg-rcore-tutorial-t2l5/src/main.rs`
- `tg-rcore-tutorial-t2l5/src/process.rs`
- `tg-rcore-tutorial-sync/src/{mutex,semaphore,condvar}.rs`
- `tg-rcore-tutorial-user/src/bin/*.rs`

我先问 AI 老师的不是“怎么一步到位实现五个原语”，而是：

> 这份代码里，哪些东西已经存在，哪些东西只是 README 里写了目标但代码根本没做？

结论很清楚：

1. `t2l5` 代码主体还是 `ch8` 基线。
2. `mutex/semaphore/condvar` 虽然接了 syscall，但 `condvar` 是简化实现。
3. 根本没有统一的实验指标和核验脚本。
4. `rwlock` 也还不存在。

所以我决定把这章做成“实验平台”，而不是“补几个 TODO 函数”。

## 第 1 轮：为什么我没有给 spinlock 额外扩 syscall

我最开始也想过直接在内核里加一个 `spin_lock` syscall。

AI 老师提醒我先问一句：

> 现有 trap 语义里，`mutex_lock` 返回失败时会发生什么？

看完 `main.rs` 后我确认：

- `MUTEX_LOCK/SEMAPHORE_DOWN/CONDVAR_WAIT`
- 只要返回 `-1`
- trap 层就会把当前线程标成 blocked

这意味着：

- 现有接口天生更适合“睡眠锁”
- 不适合直接暴露“忙等但不阻塞”的语义

如果硬扩 syscall，我还得重写返回码和 trap 分支。

所以我接受了 AI 的建议：

- `mutex/semaphore/condvar` 继续走内核同步原语
- `spinlock` 做成用户态 ticket lock
- 等待时显式 `sched_yield()`

这个取舍是刻意的，不是偷懒。它刚好适合本教程当前的协作式线程模型。

## 第 2 轮：最危险的不是没写 rwlock，而是 condvar 根本不对

一开始最“显眼”的缺口是没有 `rwlock`。

但继续读 `tg-rcore-tutorial-sync/src/condvar.rs` 之后，我马上意识到：

> 真正会把整章实验结论搞坏的，不是没有新原语，而是现有 condvar 语义就是错的。

当时那版 `wait_with_mutex` 只是：

- 解锁 mutex
- 然后立刻再试着 lock

这不是标准条件变量。

AI 老师当时给我的提醒是：

> 如果 `condvar_wait` 返回到用户态时线程并没有真正重新拿到 mutex，那用户侧的 `while (...) condvar_wait(...)` 写法根本没有语义保证。

于是我先做了真正的 condvar 修正：

1. `wait` 把 `(tid, mutex_id, mutex)` 入 condvar 队列。
2. `wait` 释放 mutex，并返回“因 unlock 唤醒了谁”。
3. `signal` 从 condvar 队列取 waiter。
4. `signal` 在内核里替 waiter 重新接入对应 mutex：
   - 如果立即拿到锁，就直接唤醒；
   - 如果拿不到，就进入 mutex wait queue。

这一步做完以后，我才敢继续写 `condvar` 的对照测试。

## 第 3 轮：指标该放在哪一层

这轮我主要和 AI 老师讨论：

> `context_switches / blocked / wakeups` 这些指标，是放在用户态统计，还是放在内核里？

最后结论是：

- `wait/hold/max_wait/starvation`
  - 放在用户态包装层，最方便按原语聚合
- `context_switches/blocked_sync_ops/wakeups`
  - 放在内核 trace 计数器，最可信

所以我在 `t2l5` 内核里加了：

- `context_switches`
  - 在线程调度器 `fetch()` 里计数
- `blocked_sync_ops`
  - 在 trap 层看到同步 syscall 返回 `-1` 时计数
- `wakeups`
  - 在 `mutex_unlock / semaphore_up / condvar_signal / condvar_wait` 的 re-enqueue 路径计数

然后在用户态做了 `sync_lab.rs`：

- `now_us()`
- `kernel_metrics()`
- `AtomicStats`
- `TicketSpinLock`
- `FairRwLock`
- `ReaderPreferRwLock`

这样我就把“内核可信数据”和“实验场景自己的统计”拆开了。

## 第 4 轮：第一次跑 success matrix，condvar 的 wait 次数居然是 0

这轮是我觉得最像“做实验”的一轮。

我第一次把 success matrix 跑完以后，看到：

- `condvar` 场景确实通过了
- 但是 `ops=0`

我立刻问 AI 老师：

> 这是不是说明 condvar 语义又坏了？

AI 没让我先怀疑实现，而是先让我看 workload：

> 你的慢操作是不是还放在 mutex 临界区里？

一看确实如此。

所以线程真正卡住的地方是：

- 抢 mutex

而不是：

- 等条件变量

我后来做了两次修正：

1. 把 consumer 的慢路径移到 `mutex_unlock()` 之后。
2. 把场景调成：
   - 小缓冲区
   - `2 producer + 1 slow consumer`

改完以后，`condvar` summary 终于变成：

- `ops=61`
- `blocked=66`

这时我才觉得“这个 condvar workload 真正碰到了条件等待”。

## 第 5 轮：Docker 不是“运行环境细节”，它本身就是个坑点

用户要求我必须通过 `~/rcore_docker.sh` 进容器。

真正跑起来之后，我陆续踩了几个坑：

### 1. `docker run -it` 没有 TTY 会直接报错

第一次我用普通管道方式调用：

- 直接报 `the input device is not a TTY`

后来我改成带 PTY 的命令才过。

### 2. 新容器里第一次 `cargo run` 不只是“编译内核”

它还会：

- rustup 同步 channel
- 下载 toolchain 组件
- 拉 crates.io index

这导致我一开始给 suite 的单 case timeout 设得太短，直接误判失败。

### 3. `build.rs` 还在试图 `cargo clone`

第一次真正进容器跑 `cargo run` 时，`t2l5/build.rs` 想在 `t2l5/` 目录下再克隆一份 `tg-rcore-tutorial-user`，然后因为没有 `cargo clone` 子命令直接炸了。

AI 老师当时提醒我：

> 先别纠结 qemu，先把 build 链路的路径假设修正。

最后我把它改成：

- 优先复用仓库里的兄弟目录 `../tg-rcore-tutorial-user`

这样才真正把 t2l5 接回了当前仓库。

## 第 6 轮：control suite 失败，原因不是对照不对，而是“宿主退出码不代表 guest 失败”

这轮也很关键。

我一开始用的是：

- `cargo run` 返回非 0

来判断对照程序失败。

但后面跑 `condvar_if_bug` 才发现：

- guest 用户程序明明 panic 了
- 但 qemu / 内核最后还是正常关机
- 宿主 `cargo run` 返回码仍然是 `0`

AI 老师当时的提醒非常直接：

> 你现在测的是“QEMU 是否正常退出”，不是“用户程序是否按预期失败”。

于是我把 control suite 分成两类：

- `timeout`
  - `spin_broken`
  - `mutex_drop_wakeup`
  - `semaphore_drop_wakeup`
- `marker`
  - `condvar_if_bug`
  - `rwlock_reader_pref`

后两类不再看宿主退出码，而是去 grep 日志里我刻意打印/触发的标记：

- `condvar if-bug failed_threads=...`
- `reader-prefer rwlock starved writer as expected`

这一步改完以后，control suite 才真正可信。

## 第 7 轮：`spin_broken` 一开始“没坏”，其实是坏得不稳定

我最早那版 `t2l5_spin_broken` 是：

- 让一个子线程拿锁后不释放

结果 control suite 里它偶尔居然跑过去了。

根因不是 ticket lock 有 bug，而是：

- 没有保证那个“坏线程”一定先拿到锁

AI 老师给我的建议很朴素：

> 既然你要做故意挂死的对照，就别把“谁先拿锁”交给调度碰运气。

所以我把它改成：

- 主线程自己先拿锁
- 永不释放
- 再创建子线程去卡死

这样 `spin_broken` 就变成稳定的 timeout 了。

## 第 8 轮：最后的结果怎么看

最后我不是盯着“哪一个平均值最小”，而是先看三件事：

1. 正确版本是不是全部稳定跑完。
2. 对照版本是不是都按预期失败。
3. `spin` vs `mutex` 的差异是不是被同一口径测出来。

结果是：

- `spinlock`
  - `blocked=0`
  - 但 `ctx_switches` 很高
- `mutex`
  - `blocked=959`
  - `ctx_switches` 明显更低

这正是这章想看到的差异。

另外：

- `condvar` 最终不是“过了就算”，而是真的跑到了 `wait`
- `rwlock` 的 fair / reader-prefer 对照也确实把 writer starvation 拉出来了

所以我最后认可这个版本的 t2l5，原因不是“代码变多了”，而是：

- 有正确实现
- 有能失败的对照
- 有统一指标
- 有 Docker + QEMU 下一键复现实验的脚本

这才像一个完整的实验交付。

## 第 9 轮：最后一个假阳性，来自 fair rwlock 的阈值

我以为所有东西都收尾了，结果真正跑默认入口 `./verify.sh` 时，`rwlock_fair` 先炸了一次。

不是实现错了，而是：

- `FairRwLock` 这版确实没有让 writer 被 reader 无限插队
- 但我给 fair 场景设的 `WRITER_STARVATION_US=24_000`
- 在一次真实的 Docker/QEMU 调度里，正常抖动就把它打成了 `starvation=2`

AI 老师这次提醒我不要把：

> “等待时间超过一个很小的经验阈值”

直接等同于：

> “算法上发生了 starvation”

于是我做了一个很小但必要的修正：

- 把 fair 场景的 starvation 阈值放宽到 `120_000us`
- 保留 `reader-prefer` 对照的激进阈值和 panic 判定

然后我先单独回归 `t2l5_rwlock_fair`，确认它重新输出：

- `[t2l5-summary] ... starvation=0`

最后再跑整套默认 `./verify.sh`，终端成功打印出：

- success table
- control table

到这一步，我才认为“脚本入口、程序输出、文档说明”三者真正一致了。
