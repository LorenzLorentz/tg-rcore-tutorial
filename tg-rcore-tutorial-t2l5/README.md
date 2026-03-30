# t2l5：同步互斥机制实验平台

这个目录现在不再是 `ch8` 的说明壳子，而是一套能直接跑的同步实验平台。

目标不是只把锁“写出来”，而是把它们变成一组可测、可比较、可做错误对照的系统：

- `spinlock`
- `mutex`
- `semaphore`
- `condvar`
- `rwlock`

## 我完成了什么

### 1. 内核侧

- 修正了 `condvar` 语义：
  - `wait` 现在会把线程挂进条件变量队列、释放互斥锁、阻塞；
  - `signal` 会把 waiter 重新接到对应 mutex 上，只有拿到锁后才回到用户态；
  - 这已经不是原来 `ch8` 那个“为了过测例的简化实现”。
- 在 `t2l5` 内核里接入了 `trace` 观测口：
  - `context_switches`
  - `blocked_sync_ops`
  - `wakeups`
- 加了两类故障注入：
  - `mutex_drop_wakeup`
  - `semaphore_drop_wakeup`

### 2. 用户态实验层

我没有为 `spinlock` 和 `rwlock` 额外扩 syscall，而是做成了用户态实验原语：

- `spinlock`
  - 用 ticket lock 实现公平自旋；
  - 在等待路径里主动 `sched_yield()`，适配本教程当前的协作式线程模型。
- `rwlock`
  - `FairRwLock`：服务队列 + 资源信号量，防止 writer 被 reader 插队饿死；
  - `ReaderPreferRwLock`：作为对照版本，故意保留 writer starvation 风险。

同时补了 `sync_lab` 辅助层，统一提供：

- 内核计数器读取
- 微秒时间戳
- 原子统计器
- ticket spinlock
- fair / reader-prefer rwlock

### 3. 经典问题

我把验收场景落成了 5 组用户程序：

- `t2l5_semaphore_pc`
  - 用 semaphore 做生产者-消费者
- `t2l5_condvar_pc`
  - 用 mutex + condvar 做生产者-消费者
- `t2l5_rwlock_fair`
  - 读者-写者
- `t2l5_phil_mutex`
  - 哲学家进餐
- `t2l5_mutex_stress` / `t2l5_spin_ticket`
  - 高竞争压力测试，用来直接比较 sleep lock 和 spin lock

### 4. 能失败的对照测试

每类原语都有对应的失败对照：

- `spinlock`
  - `t2l5_spin_broken`
  - 主线程拿锁后永不释放，子线程永久自旋
- `mutex`
  - `T2L5_FAULT_MODE=mutex_drop_wakeup`
  - waiter 被从队列里摘掉但不重新入队
- `semaphore`
  - `T2L5_FAULT_MODE=semaphore_drop_wakeup`
  - `up` 之后不唤醒等待者
- `condvar`
  - `t2l5_condvar_if_bug`
  - 两个 consumer 用 `if` 而不是 `while`，第二个线程会在条件不成立时继续往下走并 panic
- `rwlock`
  - `t2l5_rwlock_reader_pref`
  - reader-prefer 版本会把 writer 等待时间推到阈值之外，并触发 panic

## 公平性语义

### `spinlock`

- 实现：ticket lock
- 语义：FIFO 次序，bounded waiting
- 观测重点：不阻塞内核，但上下文切换数高

### `mutex`

- 实现：FIFO wait queue + handoff unlock
- 语义：不允许新线程在 unlock 后抢在队首 waiter 前面 barging
- 对照：去掉 wakeup 后直接挂死

### `semaphore`

- 实现：FIFO wait queue
- 语义：资源不足时睡眠，`up` 唤醒队首
- 对照：故意丢 wakeup

### `condvar`

- 语义：Mesa 风格
- 用户侧必须写：

```rust
while !predicate() {
    condvar_wait(...);
}
```

- 对照：`if` 版本会失败

### `rwlock`

- 正确版本：fair service queue
- 对照版本：reader-prefer
- 观测重点：writer 最大等待时间与 starvation

## 统一观测口径

所有 summary 都统一打印：

- `contention`
- `avg_wait_us`
- `max_wait_us`
- `avg_hold_us`
- `max_hold_us`
- `ctx_switches`
- `blocked`
- `wakeups`
- `starvation`

其中：

- `ctx_switches / blocked / wakeups` 来自内核 trace 计数器
- `wait / hold / starvation` 来自用户态实验包装层

## 一站式核验

### 直接跑完整套

```bash
cd tg-rcore-tutorial-t2l5
./verify.sh
```

脚本会：

1. 如果不在容器里，就通过 `~/rcore_docker.sh` 进入 Docker。
2. 先做一次 warm-up build。
3. 运行 success cases。
4. 运行 control cases。
5. 在终端打印两张表。
6. 把原始日志保存到 `.logs/suite/`。

### 只跑正确版本

```bash
./verify.sh --mode success
```

### 只跑错误对照

```bash
./verify.sh --mode control
```

`test.sh` 只是 `verify.sh` 的别名。

## 最近一次验证结果

### Success Cases

2026-03-29 在 Docker/QEMU 中执行默认入口 `./verify.sh`，success/control 两套都通过。

来自最近一次默认 `./verify.sh` 的 summary：

| case | 关键结果 |
|---|---|
| `spinlock:ticket` | `ops=960 avg_wait_us=8657 ctx_switches=15635 blocked=0 starvation=0` |
| `mutex:fifo_blocking` | `ops=960 avg_wait_us=8026 ctx_switches=7753 blocked=959 starvation=0` |
| `semaphore:producer_consumer` | `ops=240 avg_wait_us=12875 gate_blocks=120 starvation=0` |
| `condvar:producer_consumer` | `ops=61 avg_wait_us=10374 blocked=66 starvation=0` |
| `rwlock:fair` | `read_ops=600 write_ops=72 max_wait_us=5711 starvation=0` |
| `mutex:philosophers` | `ops=20 max_wait_us=735586 starvation=0` |

最重要的对比是：

- `spinlock`：`blocked=0`，但 `ctx_switches=15635`
- `mutex`：`blocked=959`，但 `ctx_switches=7753`

这正好体现了本实验想观察的差异：spin 路径不睡眠，但代价是更高的调度切换与等待成本。

### Control Cases

同一次默认 `./verify.sh` 的 control 结果：

| case | 结果 |
|---|---|
| `t2l5_spin_broken` | `timeout` |
| `mutex_drop_wakeup` | `timeout` |
| `semaphore_drop_wakeup` | `timeout` |
| `t2l5_condvar_if_bug` | 观察到 consumer 在 `ready=0` 时继续执行并 panic |
| `t2l5_rwlock_reader_pref` | 观察到 `writer_max_wait_us=1233616 starvation=1`，随后 panic |

## 关键文件

- `src/main.rs`
  - trace 计数器接线
  - sync syscall
  - 故障注入
- `src/processor.rs`
  - 内核级 `context_switches / blocked / wakeups`
- `../tg-rcore-tutorial-sync/src/condvar.rs`
  - 正确的 condvar 等待/唤醒链路
- `../tg-rcore-tutorial-user/src/sync_lab.rs`
  - 用户态实验辅助层
- `../tg-rcore-tutorial-user/src/bin/t2l5_*`
  - success / control 场景
- `verify.sh`
  - 一站式入口
- `scripts/run_suite.py`
  - 批量执行与结果判定
- `lab-notes.md`
  - 过程记录

## 过程记录

实现过程、取舍和踩坑记录在：

- `lab-notes.md`

这份记录不是事后总结，而是按“读代码 -> 动手改 -> 跑 Docker/QEMU -> 修 bug -> 再验证”的顺序写的。
