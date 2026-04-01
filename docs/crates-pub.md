# Task3 Crates Publish Record

本文档记录本次 `task3` 相关 crate 的实际发布时间顺序、版本号和关键依赖约束，便于后续继续发版时直接复用。

## 当前 crates.io 状态

截至 `2026-04-02`（本地时间），task3 相关 crate 的线上状态如下：

| crate | crates.io 状态 | 本次动作 |
|---|---|---|
| `wpj-tg-rcore-tutorial-gfx` | 已发布 `0.0.0` | 本次首次发布成功 |
| `wpj-tg-rcore-tutorial-easy-fs` | 已发布 `0.0.1` | 本次从 `0.0.0` 更新到 `0.0.1` |
| `wpj-tg-rcore-tutorial-t3l1-tangram` | 已发布 `0.0.0` | 本次首次发布成功 |
| `wpj-tg-rcore-tutorial-t3l5-pingpong` | 已发布 `0.0.0` | 本次首次发布成功 |
| `wpj-tg-rcore-tutorial-t3l8-doom` | 已发布 `0.0.0` | 本次首次发布成功 |

## 本次发布涉及的改动点

- `wpj-tg-rcore-tutorial-gfx`
  - 为 `virtio-drivers` 补上显式发布版本 `0.1.0`，避免 path 依赖在 `cargo package/publish` 阶段被拒绝。
- `wpj-tg-rcore-tutorial-easy-fs 0.0.1`
  - 新增 `FileHandle::seek`。
  - 新增 `Inode::size`。
  - 这是 `t3l8-doom` 的文件偏移调整与 WAD 读取链路所必需的扩展。
- `wpj-tg-rcore-tutorial-t3l5-pingpong`
  - 为 `virtio-drivers` 补上显式发布版本 `0.1.0`。
- `wpj-tg-rcore-tutorial-t3l8-doom`
  - 改为继续依赖官方 `tg-rcore-tutorial-syscall = 0.4.8`。
  - `LSEEK` 在本地内核 trap 分发中单独接管，不再依赖本地改过的 `tg-rcore-tutorial-syscall` path 版本。
  - task3 图形/输入相关 syscall 也在本地处理：`FRAMEBUFFER_GETINFO`、`FRAMEBUFFER_PRESENT`、`INPUT_POLL` 及其配套 `FramebufferInfo`、`InputEvent` 结构都本地化到 `t3l8-doom`。
  - `tg-easy-fs` 依赖切到 `wpj-tg-rcore-tutorial-easy-fs = 0.0.1`。
  - 为 `virtio-drivers` 和 `wpj-tg-rcore-tutorial-gfx` 补上显式版本。
  - 打包时显式包含 `capps/doom/**`、`doomgeneric/doomgeneric/**` 和包内 `tg-rcore-tutorial-user/assets/**`，避免 `cargo publish` 产物缺少 Doom 端口层和 `doom1.wad`。

## 为什么不发布 `tg-rcore-tutorial-syscall`

- crates.io 上 `tg-rcore-tutorial-syscall` 的 owner 是 `chyyuu` 和 `Ivans-11`，当前账号不具备发布权限。
- 已下载并检查 `tg-rcore-tutorial-syscall 0.4.8` 的线上源码；其中已有 `__NR_lseek` 号，但没有 task3 需要的 kernel `IO::lseek` 扩展，也没有 `FramebufferInfo`、`InputEvent`、`Platform` 及对应图形/输入 syscall 封装。
- 因此本次不再尝试发布老师包的更新版，而是在 `t3l8-doom` 内核里本地处理 `Id::LSEEK` 和 task3 图形/输入 syscall，从而保持对官方 `0.4.8` 的兼容。

## Task3 直接依赖关系

| crate | 直接依赖 |
|---|---|
| `wpj-tg-rcore-tutorial-gfx 0.0.0` | `virtio-drivers = 0.1.0` |
| `wpj-tg-rcore-tutorial-easy-fs 0.0.1` | `spin`, `bitflags` |
| `wpj-tg-rcore-tutorial-t3l1-tangram 0.0.0` | `tg-rcore-tutorial-sbi = 0.4.8`, `wpj-tg-rcore-tutorial-gfx = 0.0.0` |
| `wpj-tg-rcore-tutorial-t3l5-pingpong 0.0.0` | `tg-rcore-tutorial-ch5` 同级依赖 + `virtio-drivers = 0.1.0` + `wpj-tg-rcore-tutorial-gfx = 0.0.0` |
| `wpj-tg-rcore-tutorial-t3l8-doom 0.0.0` | `tg-rcore-tutorial-ch8` 同级依赖 + `virtio-drivers = 0.1.0` + `wpj-tg-rcore-tutorial-gfx = 0.0.0` + `wpj-tg-rcore-tutorial-easy-fs = 0.0.1` |

## 实际发布顺序与结果

1. `wpj-tg-rcore-tutorial-easy-fs 0.0.1`
2. `wpj-tg-rcore-tutorial-gfx 0.0.0`
3. `wpj-tg-rcore-tutorial-t3l1-tangram 0.0.0`
4. `wpj-tg-rcore-tutorial-t3l5-pingpong 0.0.0`
5. `wpj-tg-rcore-tutorial-t3l8-doom 0.0.0`

上述 5 次发布均已成功完成，线上版本检查结果与本地预期一致。

## 打包与发布注意事项

- 本机 `~/.cargo/config` 把 `crates-io` 重定向到了 TUNA mirror；`cargo publish` 时需要显式改回官方 `crates.io`。
- 当前 shell 环境里存在失效的 `http_proxy/https_proxy`，发布前需要清掉。
- `t3l5` 和 `t3l8` 的用户态程序都随各自 crate 打包发布，不需要额外发布独立 user crate。
- `t3l8-doom` 的打包结果必须检查 tarball 中是否包含：
  - `capps/doom/**`
  - `doomgeneric/doomgeneric/**`
  - `tg-rcore-tutorial-user/assets/doom1.wad`
