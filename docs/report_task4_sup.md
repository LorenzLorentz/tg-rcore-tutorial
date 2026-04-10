## TASK4 补充报告：Kernel Interruptible but Not Fully Preemptible

### 1. 这份补充报告说明什么

这个文档实现 `t4l45` 里缺失的内核中断。

这里说的目标语义是：

1. 内核执行 syscall、信号处理等 S 态代码时，timer interrupt 可以真的打断它；
2. 中断处理完以后，必须先回到同一个内核控制流继续执行；
3. 是否调度，不在中断上下文里决定，而是延后到一个明确的 **safe return point**；

### 2. 实现

我实现的是：

1. 内核在执行 syscall 和 trap-return 前的信号处理时，临时打开 `sstatus.SIE`；
2. 如果这期间来了 `SupervisorTimer`，会进入新的常驻 kernel trap entry；
3. trap handler 只做最小工作：
   - 保存当前内核现场；
   - 重新编程下一次 timer；
   - 记录一个 deferred tick；
   - 直接 `sret` 回被打断的内核位置；
4. 等当前 syscall / signal 路径做完，准备返回用户态时，再检查 deferred ticks；
5. 如果这些 ticks 按调度策略要求让出 CPU，才在这里真正切换任务。

它不是：

1. “任何内核指令点都能直接被切走”的 fully preemptible kernel；
2. “中断上下文里直接拿 runqueue 锁并 schedule”的硬抢占模型；
3. “已经有 per-thread kernel stack / kernel context，线程可以在内核半路被换下后将来从内核里续跑”的实现。

### 3. Safe Return Point 是怎么定的

这次实现里，我把 safe return point 明确定在：

1. 当前 trap 已经回到 Rust 顶层分发逻辑；
2. syscall 已经处理完；
3. signal 注入决策已经完成；
4. 当前线程即将返回用户态之前。

在这个点做调度，有几个好处：

1. trap 上下文已经是自洽的；
2. syscall 内部临时状态已经收束；
3. 不需要在中断上下文里拿调度器锁；
4. 不需要实现“从内核半路恢复线程”的完整 kernel context 机制。

对应代码上，这个安全点被统一收束在：

1. [`should_preempt_before_user_return()`](/Users/lorenzlorentz/tg-rcore-tutorial/tg-rcore-tutorial-t4l45/src/main.rs#L321)
2. `run_scheduler_loop()` 里各类 trap 返回后的分支判断  
   文件：[`main.rs`](/Users/lorenzlorentz/tg-rcore-tutorial/tg-rcore-tutorial-t4l45/src/main.rs#L330)

### 3. 具体修改

#### 3.1 新增常驻 kernel trap 模块

我新增了 [`tg-rcore-tutorial-t4l45/src/trap.rs`](/Users/lorenzlorentz/tg-rcore-tutorial/tg-rcore-tutorial-t4l45/src/trap.rs)。

这个模块做了三件核心事情。

##### 3.1.1 每 hart 维护一个 deferred tick 计数器

在 [`HartTrapState`](/Users/lorenzlorentz/tg-rcore-tutorial/tg-rcore-tutorial-t4l45/src/trap.rs#L4) 中，我只保留了一个最小状态: `pending_timer_ticks: AtomicUsize`

它表示“当前 hart 在内核执行期间被 timer 打断了多少次，但这些 ticks 还没有在安全点交给调度器处理”。

这样做的关键目的，是把中断响应和调度决策拆开。

前者可以在 IRQ 上下文里立即完成，后者则延后到安全点。

##### 3.1.2 每个 hart 启动后安装常驻 `stvec`

在 [`init_hart()`](/Users/lorenzlorentz/tg-rcore-tutorial/tg-rcore-tutorial-t4l45/src/trap.rs#L39) 里，我会：

1. 清零当前 hart 的 `pending_timer_ticks`；
2. 把 `stvec` 指到新的 `kernel_trap_entry`。

这个初始化是在 [`rust_main()`](/Users/lorenzlorentz/tg-rcore-tutorial/tg-rcore-tutorial-t4l45/src/main.rs#L225) 里、激活内核页表之后调用的：

1. `activate_kernel_space()`
2. `trap::init_hart(hart_id)`
3. `enable_timer_interrupts()`

这样之后 CPU 只要处在普通内核执行期，并且 `SIE` 打开，就会走这条常驻入口。

##### 3.1.3 `kernel_trap_entry` 只做最小工作并返回原内核流

新的 [`kernel_trap_entry()`](/Users/lorenzlorentz/tg-rcore-tutorial/tg-rcore-tutorial-t4l45/src/trap.rs#L124) 是一段裸汇编入口。它会：

1. 在当前内核栈上分配一个 `KernelTrapFrame`；
2. 保存通用寄存器、`sstatus`、`sepc`、`scause`、`stval`；
3. 调用 [`kernel_trap_rust()`](/Users/lorenzlorentz/tg-rcore-tutorial/tg-rcore-tutorial-t4l45/src/trap.rs#L104)；
4. 恢复 `sstatus` / `sepc` / 通用寄存器；
5. 直接 `sret` 回原来的内核位置。

这里最重要的是：**没有在 trap handler 里调用调度器，也没有在 trap handler 里改当前线程状态。**

`kernel_trap_rust()` 目前只接受一种内核态异步中断: `SupervisorTimer`

它会：

1. 调用 [`arm_next_timer()`](/Users/lorenzlorentz/tg-rcore-tutorial/tg-rcore-tutorial-t4l45/src/main.rs#L293) 重新编程下一次 timer；
2. 把当前 hart 的 `pending_timer_ticks += 1`。

其余内核 trap 仍然直接 panic。这是刻意保守的做法，因为这次目标不是把所有 S 态异常路径都推广成成熟子系统，而是先把“内核可被 timer 中断”这条主线做好。

#### 3.2 `stvec`的保存与恢复

在 [`tg-kernel-context-mp/src/lib.rs`](/Users/lorenzlorentz/tg-rcore-tutorial/tg-rcore-tutorial-kernel-context-mp/src/lib.rs#L146) 里，我补了对旧 `stvec` 的保存与恢复：

1. 进入 `execute_naked()` 前先 `csrr old_stvec, stvec`；
2. 返回后 `csrw stvec, old_stvec`。

这样做的效果是：

1. 用户线程执行期间，`LocalContext::execute()` 仍然可以临时把 `stvec` 指到自己的 trap return stub；
2. 但一旦返回普通内核代码，`stvec` 会恢复成当前 hart 的常驻 kernel trap vector；
3. 于是内核执行期间再次到来的异步中断，就能进入正确的常驻入口，而不是误入用户 trap return stub。

#### 3.3 中断开启区间

我没有把整个内核都改成默认开中断，而是只在一个受控区间内临时打开：

1. syscall 分发；
2. trap-return 前的信号处理。

它会：

1. 读取进入前的 `sstatus.SIE`；
2. 如果原本关闭，就临时 `set_sie()`；
3. 执行闭包；
4. 退出时如果原本是关闭的，就恢复成关闭。

这样 timer interrupt 就可以真实地打断内核里的 syscall / signal 路径，但不会把整个调度循环、idle 自旋等路径都默认暴露在可中断区里。

#### 3.4 调度决策

抢占决策

1. 取走当前 hart 上所有 `pending_timer_ticks`；
2. 再加上“这次 trap 返回本身携带的那一个 tick”，例如用户态直接被 timer 打断时的 `extra_ticks = 1`；
3. 调用 `ProcessorInner::handle_ticks(task, total_ticks)`；
4. 由调度策略决定这些 ticks 是否足以触发让出 CPU。

这样如果内核在一次 syscall 里被 timer 连续打断多次, 这些 ticks 不会丢, 但也不会在 IRQ 上下文里直接 schedule。

#### 3.5 返回语义

这次我还改了一个对整体语义影响很大的点：

原来几乎所有普通 syscall 都会在返回后走 `suspend_running_task()`。现在改成：

1. `EXIT`：退出；
2. `SEMAPHORE_DOWN / MUTEX_LOCK / CONDVAR_WAIT` 返回 `-1`：阻塞；
3. `SCHED_YIELD`：显式让出；
4. timer tick 命中时间片：抢占；
5. 其余普通 syscall：默认继续当前线程，直接回用户态。

### 4. pipeline

现在可以把整个流程概括成下面这条状态机：

```text
用户态运行
-> timer interrupt
-> 回到调度循环
-> 需要切换则 schedule，不需要则继续该线程

用户态 ecall
-> 进入调度循环的 syscall 分支
-> 临时打开内核中断
-> 若期间来了 timer interrupt：
   - 进入 kernel_trap_entry
   - 保存当前内核现场
   - 重新编程 timer
   - pending_timer_ticks += 1
   - 返回原 syscall 流继续执行
-> syscall 和 signal 处理结束
-> 在 safe return point 检查 pending_timer_ticks
-> 需要切换则 schedule，否则返回用户态
```

也就是说：

1. 内核可以被打断；
2. 但中断后一定先回原内核流；
3. 调度决策延后；
4. 不在 IRQ top-half 里 schedule。

这正是“kernel interruptible but not fully preemptible”的实现语义。

### 5. 总结

这次补充实现完成后，`t4l45` 的 trap / timer / 调度语义比之前更接近一个真正的内核，而不再只是“用户态 trap 回来后顺便处理一下 timer”的实验骨架。

所以，如果用一句话概括这次补充工作，我会说：

**我把 `t4l45` 从“只有用户态真正参与异步 timer 驱动”的 trap 结构，推进成了“内核也可被 timer 中断，但调度延后到安全点”的实现的 trap 脚手架。**
