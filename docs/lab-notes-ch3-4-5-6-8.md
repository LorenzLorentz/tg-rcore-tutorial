# ch3 / ch4 / ch5 / ch6 / ch8 实验记录

## 1. 环境启动

我在仓库根目录下使用下面的命令进入带模拟器的实验环境：

```bash
cd /Users/lorenzlorentz/tg-rcore-tutorial
~/rcore_docker.sh bash
```

进入容器后，如果想直接复用我这次测试时准备好的本地工具和缓存，可以再执行：

```bash
export RUSTUP_HOME=/workspace/.rustup-home
export PATH="/workspace/.cargo-home/bin:$PATH"
```

然后在对应章节目录里运行：

```bash
./test.sh exercise
```

本次验证日志保存在：

- `.logs/ch3-exercise.log`
- `.logs/ch4-exercise.log`
- `.logs/ch5-exercise.log`
- `.logs/ch6-exercise.log`
- `.logs/ch8-exercise.log`

## 2. 过程观察

### ch3

我先实现了 `sys_trace` 的三个功能：读用户地址、写用户地址、查询 syscall 次数。

一开始我直接给每个 `TaskControlBlock` 放了一个 512 项的 syscall 计数数组，结果 `rust_main` 里那个 `tcbs` 数组本来就在内核栈上，大小瞬间爆掉。现象不是直接 panic，而是 QEMU 启动后疯狂输出空字节，说明启动栈已经被我自己踩坏了。

这个问题提醒我：第三章的数据结构虽然看起来简单，但它们的放置位置非常关键。最后我把计数结构改成了一个很小的稀疏表，只记录实际出现过的 syscall id，既满足测例，也不再把栈顶爆。

### ch4

这一章我实现了：

- 基于地址翻译的 `trace`
- `mmap`
- `munmap`

一开始我把 ch3 的“大数组计数法”搬到了 `Process` 里，结果 `ch4` 又卡在启动阶段，只打出 `detect app[0]`。这里虽然进程放在 `Vec` 里，但启动路径上的局部对象仍然会占用 24 KiB 启动栈，所以这个问题本质上还是“对象太胖”。

另一个更隐蔽的问题出现在非法地址测试上：`trace_read(isize::MAX as *const u8)` 理应返回 `None`，但我第一次实现时返回了 `Some(0)`。根因是 `VAddr::new()` 会把高位非法地址折叠进 Sv39 地址空间里，所以我额外补了“只允许低 39 位用户地址”的检查，再去做 `translate()`。

### ch5

这一章我实现了：

- `spawn`
- `set_priority`
- stride 调度
- `mmap`
- `munmap`

我这里最重要的观察是：stride 调度的核心不是“每次加个值”这么简单，而是要保证“选当前最小 stride 的进程，再按 `BIG_STRIDE / priority` 增加它的 stride”。一旦调度顺序和步长更新位置错了，`ch5_stride` 的比例就会明显跑偏。

### ch6

这一章除了沿用 ch5 的进程/调度逻辑外，还补了文件系统练习：

- `linkat`
- `unlinkat`
- `fstat`
- easy-fs 的 inode link count 支持

这里最容易错的是“目录项删除”和“真正释放 inode”的关系。我的实现思路是：只有 link count 归零时才真正回收 inode；否则只是删目录项，不应该把文件内容直接释放。

### ch8

这一章我实现了死锁检测开关和两类死锁检测：

- semaphore 资源分配图检测
- mutex 等待环检测

我在实现时最注意的点有两个：

- 检测必须发生在真正阻塞之前
- 唤醒路径不仅要唤醒线程，还要同步更新“谁持有资源”的 bookkeeping

如果只做前者不做后者，死锁检测状态会越来越假，后面的测例会误判。

## 3. 具体实现思路

### ch3

- 在 `tg-rcore-tutorial-ch3/src/task.rs` 中实现 `trace`
- syscall 计数放在 TCB 内的小型稀疏表里
- 在 `handle_syscall()` 入口先计数，再分发 syscall

### ch4

- 在 `tg-rcore-tutorial-ch4/src/main.rs` 中实现 `trace/mmap/munmap`
- 在 `tg-rcore-tutorial-ch4/src/process.rs` 中加入轻量 syscall 计数接口
- `trace` 先做用户地址合法性检查，再做 `translate()`

### ch5

- 在 `tg-rcore-tutorial-ch5/src/processor.rs` 中改为 stride 调度
- 在 `tg-rcore-tutorial-ch5/src/main.rs` 中实现 `spawn/set_priority/mmap/munmap`
- 在 `tg-rcore-tutorial-ch5/src/process.rs` 中补 `stride/priority`

### ch6

- 在 `tg-rcore-tutorial-easy-fs/src/layout.rs` 增加 `nlink`
- 在 `tg-rcore-tutorial-easy-fs/src/efs.rs` 和 `tg-rcore-tutorial-easy-fs/src/vfs.rs` 提供 link count 与 inode 回收接口
- 在 `tg-rcore-tutorial-ch6/src/fs.rs` 中实现 `link/unlink`
- 在 `tg-rcore-tutorial-ch6/src/main.rs` 中实现 `linkat/unlinkat/fstat`

### ch8

- 在 `tg-rcore-tutorial-ch8/src/process.rs` 中加入 deadlock state
- 在 `tg-rcore-tutorial-ch8/src/main.rs` 中实现
  - `enable_deadlock_detect`
  - `mutex_lock/mutex_unlock`
  - `semaphore_down/semaphore_up`
- 检测到死锁时返回 `-0xdead`

## 4. 环境侧额外处理

实验容器里缺少 `cargo-clone`，而这些章节的 `build.rs` 默认会依赖它拉取 `tg-rcore-tutorial-user`。为了避免在容器里继续装一堆系统依赖，我做了两件事：

- 把 `tg-rcore-tutorial-user@0.4.8` 解压到 `tg-rcore-tutorial-ch3/ch4/ch5/ch6/ch8/tg-rcore-tutorial-user`
- 把 `tg-rcore-tutorial-checker` 安装到工作区内的 `.cargo-home/bin`

因此仓库里现在会看到：

- `.cargo-home/`
- `.rustup-home/`
- `tg-rcore-tutorial-ch3/tg-rcore-tutorial-user/`
- `tg-rcore-tutorial-ch4/tg-rcore-tutorial-user/`
- `tg-rcore-tutorial-ch5/tg-rcore-tutorial-user/`
- `tg-rcore-tutorial-ch6/tg-rcore-tutorial-user/`
- `tg-rcore-tutorial-ch8/tg-rcore-tutorial-user/`

这些目录的作用都是“让 Docker 容器里的测试可复用、可离线继续跑”。

## 5. 最终验证

我在 Docker 环境中逐章执行了下面的命令：

```bash
cd /workspace/tg-rcore-tutorial-ch3 && ./test.sh exercise
cd /workspace/tg-rcore-tutorial-ch4 && ./test.sh exercise
cd /workspace/tg-rcore-tutorial-ch5 && ./test.sh exercise
cd /workspace/tg-rcore-tutorial-ch6 && ./test.sh exercise
cd /workspace/tg-rcore-tutorial-ch8 && ./test.sh exercise
```

结果：

- ch3: 通过
- ch4: 通过
- ch5: 通过
- ch6: 通过
- ch8: 通过

我认为这 5 个实验现在已经完成，而且过程中的关键问题和修正理由都记录下来了。
