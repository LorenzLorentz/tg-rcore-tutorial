# Task3/Task4 Crates Publish Record

本文档记录本次 `task3` 相关 crate 的实际发布时间顺序、版本号和关键依赖约束，便于后续继续发版时直接复用。

## 当前 crates.io 状态

截至 `2026-04-02`（本地时间），task3 相关 crate 的线上状态如下：

| crate | crates.io 状态 | 本次动作 |
|---|---|---|
| `wpj-tg-rcore-tutorial-gfx` | 已发布 `0.0.0` | 本次首次发布成功 |
| `wpj-tg-rcore-tutorial-easy-fs` | 已发布 `0.0.1` | 本次从 `0.0.0` 更新到 `0.0.1` |
| `wpj-tg-rcore-tutorial-t3l1-tangram` | 已发布 `0.0.0` | 本次首次发布成功 |
| `wpj-tg-rcore-tutorial-t3l5-pingpong` | 已发布 `0.0.1` | 首发 `0.0.0` 后，本次补丁更新到 `0.0.1` |
| `wpj-tg-rcore-tutorial-t3l8-doom` | 已发布 `0.0.2` | 在 `0.0.1` 基础上继续补丁更新到 `0.0.2` |

## 本次发布涉及的改动点

- `wpj-tg-rcore-tutorial-gfx`
  - 为 `virtio-drivers` 补上显式发布版本 `0.1.0`，避免 path 依赖在 `cargo package/publish` 阶段被拒绝。
- `wpj-tg-rcore-tutorial-easy-fs 0.0.1`
  - 新增 `FileHandle::seek`。
  - 新增 `Inode::size`。
  - 这是 `t3l8-doom` 的文件偏移调整与 WAD 读取链路所必需的扩展。
- `wpj-tg-rcore-tutorial-t3l5-pingpong`
  - 为 `virtio-drivers` 补上显式发布版本 `0.1.0`。
  - `0.0.1` 额外修复了内嵌 `tg-rcore-tutorial-user` manifest 的构建回归：发布包继续保留 `Cargo.user.toml`，但本地构建时由 `build.rs` 临时生成真实的 `Cargo.toml`，从而恢复 `cargo build/run`。
- `wpj-tg-rcore-tutorial-t3l8-doom`
  - 改为继续依赖官方 `tg-rcore-tutorial-syscall = 0.4.8`。
  - `LSEEK` 在本地内核 trap 分发中单独接管，不再依赖本地改过的 `tg-rcore-tutorial-syscall` path 版本。
  - task3 图形/输入相关 syscall 也在本地处理：`FRAMEBUFFER_GETINFO`、`FRAMEBUFFER_PRESENT`、`INPUT_POLL` 及其配套 `FramebufferInfo`、`InputEvent` 结构都本地化到 `t3l8-doom`。
  - `tg-easy-fs` 依赖切到 `wpj-tg-rcore-tutorial-easy-fs = 0.0.1`。
  - 为 `virtio-drivers` 和 `wpj-tg-rcore-tutorial-gfx` 补上显式版本。
  - 打包时显式包含 `capps/doom/**`、`doomgeneric/doomgeneric/**` 和包内 `tg-rcore-tutorial-user/assets/**`，避免 `cargo publish` 产物缺少 Doom 端口层和 `doom1.wad`。
  - `0.0.1` 额外修复了与 `t3l5` 相同的内嵌 manifest 构建回归，同时继续保证打包结果包含 `doomgeneric/**` 和 `doom1.wad`。
  - `0.0.2` 额外修复了 Doom 默认启动行为与交互说明：
    - 默认附加 `-skill 2 -warp 1 1`，启动后直接进入新游戏，不再落到 attract/demo 播放。
    - 菜单继续以 `W/A/S/D` 为默认导航，同时兼容方向键。
    - 启动后在命令行打印当前键位说明。
    - `build.rs` 改为递归跟踪 `capps/doom/**` 与 `doomgeneric/**` 的文件变化，避免修改 C 端口后未触发重编译。

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
| `wpj-tg-rcore-tutorial-t3l5-pingpong 0.0.1` | `tg-rcore-tutorial-ch5` 同级依赖 + `virtio-drivers = 0.1.0` + `wpj-tg-rcore-tutorial-gfx = 0.0.0` |
| `wpj-tg-rcore-tutorial-t3l8-doom 0.0.2` | `tg-rcore-tutorial-ch8` 同级依赖 + `virtio-drivers = 0.1.0` + `wpj-tg-rcore-tutorial-gfx = 0.0.0` + `wpj-tg-rcore-tutorial-easy-fs = 0.0.1` |

## 实际发布顺序与结果

1. `wpj-tg-rcore-tutorial-easy-fs 0.0.1`
2. `wpj-tg-rcore-tutorial-gfx 0.0.0`
3. `wpj-tg-rcore-tutorial-t3l1-tangram 0.0.0`
4. `wpj-tg-rcore-tutorial-t3l5-pingpong 0.0.0`
5. `wpj-tg-rcore-tutorial-t3l8-doom 0.0.0`
6. `wpj-tg-rcore-tutorial-t3l5-pingpong 0.0.1`
7. `wpj-tg-rcore-tutorial-t3l8-doom 0.0.1`
8. `wpj-tg-rcore-tutorial-t3l8-doom 0.0.2`

上述 8 次发布均已成功完成，线上版本检查结果与本地预期一致。

## 打包与发布注意事项

- 本机 `~/.cargo/config` 把 `crates-io` 重定向到了 TUNA mirror；`cargo publish` 时需要显式改回官方 `crates.io`。
- 当前 shell 环境里存在失效的 `http_proxy/https_proxy`，发布前需要清掉。
- `t3l5` 和 `t3l8` 的用户态程序都随各自 crate 打包发布，不需要额外发布独立 user crate。
- `t3l8-doom` 的打包结果必须检查 tarball 中是否包含：
  - `capps/doom/**`
  - `doomgeneric/doomgeneric/**`
  - `tg-rcore-tutorial-user/assets/doom1.wad`

# Task4 Crates Publish Record

本文补充记录 `task4` 相关 crate 的实际发布情况。和 `task3` 不同，`task4` 除了 `baseline` 和最终 `t4l45` 之外，还需要把多核版基础组件拆成独立的 `-mp` crate，保证 crates.io 上的依赖闭包自洽。

## 当前 crates.io 状态

截至 `2026-04-04`（本地时间），`task4` 相关 crate 的线上状态如下：

| crate | crates.io 状态 | 本次动作 |
|---|---|---|
| `tg-rcore-tutorial-kernel-alloc-mp` | 已发布 `0.4.8` | 本次首次发布成功 |
| `tg-rcore-tutorial-sbi-mp` | 已发布 `0.4.8` | 本次首次发布成功 |
| `wpj-tg-rcore-tutorial-sync-mp` | 已发布 `0.0.0` | 本次首次发布成功 |
| `tg-rcore-tutorial-kernel-context-mp` | 已发布 `0.4.8` | 本次首次发布成功 |
| `tg-rcore-tutorial-signal-mp` | 已发布 `0.4.8` | 本次首次发布成功 |
| `tg-rcore-tutorial-signal-impl-mp` | 已发布 `0.4.8` | 本次首次发布成功 |
| `wpj-tg-rcore-tutorial-t4l45-baseline` | 已发布 `0.0.0` | 本次首次发布成功 |
| `wpj-tg-rcore-tutorial-t4l45` | 已发布 `0.0.0` | 本次首次发布成功 |
| `wpj-tg-rcore-tutorial-user` | 已发布 `0.0.0` | 本次未重发，沿用线上已有版本 |
| `wpj-tg-rcore-tutorial-sync` | 已发布 `0.0.0` | 本次未重发，baseline 继续沿用 |

## 为什么 `task4` 需要补发更多 `-mp` 组件

最开始只把 `sbi/kernel-alloc/sync` 做成了 `-mp` 包，但在发布 `t4l45` 时，`cargo publish` 的官方验证链路暴露出一个更深的依赖事实：

- `t4l45` 直接使用了本地修改过 API 的 `tg-rcore-tutorial-kernel-context`
- `tg-rcore-tutorial-signal` 和 `tg-rcore-tutorial-signal-impl` 又通过 `LocalContext` 把这条类型链继续向上传递

如果不把这三者也发布成独立的 `-mp` 组件，`t4l45` 的发布包在 crates.io 环境中会回退到官方旧版 `0.4.8`，从而在 `ForeignContext::execute` 的接口上直接编译失败。

因此，最终 `t4l45` 的多核依赖闭包是：

- `tg-rcore-tutorial-sbi-mp = 0.4.8`
- `tg-rcore-tutorial-kernel-alloc-mp = 0.4.8`
- `wpj-tg-rcore-tutorial-sync-mp = 0.0.0`
- `tg-rcore-tutorial-kernel-context-mp = 0.4.8`
- `tg-rcore-tutorial-signal-mp = 0.4.8`
- `tg-rcore-tutorial-signal-impl-mp = 0.4.8`

而 `wpj-tg-rcore-tutorial-t4l45-baseline 0.0.0` 仍然保持依赖原始的不带后缀组件。

## 实际发布顺序与结果

本次 `task4` 的实际发布顺序如下：

1. `tg-rcore-tutorial-kernel-alloc-mp 0.4.8`
2. `tg-rcore-tutorial-sbi-mp 0.4.8`
3. `wpj-tg-rcore-tutorial-sync-mp 0.0.0`
4. `wpj-tg-rcore-tutorial-t4l45-baseline 0.0.0`
5. `tg-rcore-tutorial-kernel-context-mp 0.4.8`
6. `tg-rcore-tutorial-signal-mp 0.4.8`
7. `tg-rcore-tutorial-signal-impl-mp 0.4.8`
8. `wpj-tg-rcore-tutorial-t4l45 0.0.0`

上述 8 次发布均已成功完成，随后用 `cargo search --registry crates-io` 对这些 crate 做了官方索引可见性核对，结果与本地预期一致。

## 发布过程中的关键问题

- `t4l45` 的最初发布失败并不是网络问题，而是依赖链不完整：
  - 只发布 `sbi/kernel-alloc/sync` 的 `-mp` 版本不够，`kernel-context/signal/signal-impl` 也必须补成 `-mp`
- 本机 `~/.cargo/config` 把 `crates-io` 重定向到了 TUNA mirror：
  - 发布时需要临时把这个文件移开，直接对官方 `crates.io` 操作
  - 否则刚刚新发出的 crate 在镜像还没同步时，后续发布会因为“找不到新依赖”而失败
- 当前 shell 环境里存在失效的 `http_proxy/https_proxy`：
  - 发布和核对前都需要先清掉
- crates.io 对“新包”有严格限流：
  - `tg-rcore-tutorial-signal-mp` 首次尝试被限制到 `2026-04-04 04:49:48 CST`
  - `tg-rcore-tutorial-signal-impl-mp` 首次尝试被限制到 `2026-04-04 04:59:48 CST`
  - `wpj-tg-rcore-tutorial-t4l45` 首次尝试被限制到 `2026-04-04 05:09:48 CST`
  - 因此本次最终采用“按服务端返回的绝对时间点精确重试”的方式完成全部发布

## Task4 发布注意事项

- 如果后续还要继续发 `task4` 相关的新 crate，最好预期 crates.io 的新包限流是“分钟级”的，而不是一次性可以连发很多个
- 如果只是更新已经存在的 crate，通常不会像“全新 crate 名”那样频繁触发这类限流
- `t4l45` 的用户态程序仍然随 `t4l45` crate 一起打包，不需要再额外单独发布独立的 user 包
