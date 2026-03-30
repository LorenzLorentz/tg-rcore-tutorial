# WPJ Publish Dependency Map

本文档记录当前 `wpj-*` 发布方案中需要发布的包，以及它们对老师 crates.io 包和本地 `wpj-*` 包的依赖关系。

token: <expired-by-user>

## 需要发布的包

- `wpj-tg-rcore-tutorial-user`
- `wpj-tg-rcore-tutorial-gfx`
- `wpj-tg-rcore-tutorial-easy-fs`
- `wpj-tg-rcore-tutorial-sync`
- `wpj-tg-rcore-tutorial-ch3`
- `wpj-tg-rcore-tutorial-ch4`
- `wpj-tg-rcore-tutorial-ch5`
- `wpj-tg-rcore-tutorial-ch6`
- `wpj-tg-rcore-tutorial-ch8`
- `wpj-tg-rcore-tutorial-t2l4`
- `wpj-tg-rcore-tutorial-t2l5`
- `wpj-tg-rcore-tutorial-t3l1-tangram`
- `wpj-tg-rcore-tutorial-t3l5-pingpong`
- `wpj-tg-rcore-tutorial-t3l8-doom`

## 依赖来源约定

- 老师 crates.io 包：`tg-rcore-tutorial-*`
- 我们发布的包：`wpj-tg-rcore-tutorial-*`
- `t3l5` 和 `t3l8` 的用户态程序随各自 crate 一起打包，不单独发布

## 直接依赖关系

| 包 | 直接依赖 |
|---|---|
| `wpj-tg-rcore-tutorial-user` | `tg-rcore-tutorial-console`, `tg-rcore-tutorial-syscall` |
| `wpj-tg-rcore-tutorial-gfx` | `virtio-drivers` |
| `wpj-tg-rcore-tutorial-ch3` | `tg-rcore-tutorial-sbi`, `tg-rcore-tutorial-linker`, `tg-rcore-tutorial-console`, `tg-rcore-tutorial-kernel-context`, `tg-rcore-tutorial-syscall`; 构建期拉 `tg-rcore-tutorial-user` |
| `wpj-tg-rcore-tutorial-ch4` | `ch3` 依赖 + `tg-rcore-tutorial-kernel-alloc`, `tg-rcore-tutorial-kernel-vm`; 构建期拉 `tg-rcore-tutorial-user` |
| `wpj-tg-rcore-tutorial-ch5` | `ch4` 依赖 + `tg-rcore-tutorial-task-manage(proc)`; 构建期拉 `tg-rcore-tutorial-user` |
| `wpj-tg-rcore-tutorial-easy-fs` | `spin`, `bitflags` |
| `wpj-tg-rcore-tutorial-sync` | `riscv`, `spin`, `tg-rcore-tutorial-task-manage` |
| `wpj-tg-rcore-tutorial-ch6` | `ch5` 依赖 + `wpj-tg-rcore-tutorial-easy-fs`; 构建期拉 `tg-rcore-tutorial-user` |
| `wpj-tg-rcore-tutorial-ch8` | `ch6` 依赖 + `tg-rcore-tutorial-signal`, `tg-rcore-tutorial-signal-impl`, `tg-rcore-tutorial-sync`, `tg-rcore-tutorial-task-manage(thread)`; 构建期拉 `tg-rcore-tutorial-user` |
| `wpj-tg-rcore-tutorial-t2l4` | `ch8` 同级依赖；构建期拉 `wpj-tg-rcore-tutorial-user` |
| `wpj-tg-rcore-tutorial-t2l5` | `ch8` 同级依赖，但 `tg-rcore-tutorial-sync` 改为 `wpj-tg-rcore-tutorial-sync`；构建期拉 `wpj-tg-rcore-tutorial-user` |
| `wpj-tg-rcore-tutorial-t3l1-tangram` | `tg-rcore-tutorial-sbi`, `wpj-tg-rcore-tutorial-gfx` |
| `wpj-tg-rcore-tutorial-t3l5-pingpong` | `ch5` 同级依赖；用户态程序随包内 `tg-rcore-tutorial-user/` 一起构建 |
| `wpj-tg-rcore-tutorial-t3l8-doom` | `ch8` 同级依赖；用户态程序随包内 `tg-rcore-tutorial-user/` 一起构建 |

## 为什么 `t2` 需要 `wpj-user`

- `t2l4` 的 workload `sched_lab_*` 只存在于根 `tg-rcore-tutorial-user`
- `t2l5` 的 workload `t2l5_*` 也只存在于根 `tg-rcore-tutorial-user`
- 因此 `t2` 不能直接拉老师原版 `tg-rcore-tutorial-user`，必须拉我们自己的 `wpj-tg-rcore-tutorial-user`

## 为什么还需要额外发布 `wpj-easy-fs` 和 `wpj-sync`

- `ch6` 当前代码使用了 `inode_id`、`nlink`、`create_hard_link`、`remove_link` 等扩展文件系统接口，老师 crates.io 的 `tg-rcore-tutorial-easy-fs 0.4.8` 不包含这些 API
- `t2l5` 当前代码使用了扩展版条件变量与互斥锁接口，老师 crates.io 的 `tg-rcore-tutorial-sync 0.4.8` 接口形状不同
- 实测下，`ch3/ch4/ch5/ch8/t2l4` 可以继续直接依赖老师 crates.io 基础包；只有 `ch6` 和 `t2l5` 需要这两个额外 `wpj-*` 基础包

## 为什么 `t3` 不需要额外发布 user crate

- `t3l5-pingpong` 自带 `tg-rcore-tutorial-user/`
- `t3l8-doom` 自带 `tg-rcore-tutorial-user/`
- 两者的 `build.rs` 会优先使用包内用户态程序目录
- 因此 `t3` 发布时不需要再额外依赖 `wpj-tg-rcore-tutorial-user`
