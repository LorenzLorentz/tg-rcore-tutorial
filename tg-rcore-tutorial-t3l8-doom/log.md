# Doom 移植开发记录

## 说明

本文档记录在 `tg-rcore-tutorial-t3l8-doom` 中完成 Doom 移植时，我实际面临过的工程选择、做出这些选择的原因，以及遇到的主要困难、定位过程和解决办法。

这份记录不是泛泛而谈的“总结”，而是围绕本次移植真正发生过的问题来写，尽量保留技术决策背后的约束条件与取舍逻辑，便于后续复盘、验收和继续维护。

---

## 一、我面临过哪些选择，以及为什么这样选

### 1. 选择“保留 Doom 为纯 C 用户程序”，而不是重写成 Rust

这是最先需要确定的方向。

当时有两个可能路径：

- 路径 A：把 Doom 当成一个真正的 C 用户程序，在现有操作系统之上运行；
- 路径 B：把 Doom 的关键部分改写或重包成 Rust，尽量贴近现有 tutorial 用户程序体系。

我选择了路径 A。

原因：

- 题目已经明确说明这是一个“纯 C 游戏”；
- 这次任务的重点不是“复刻 Doom 逻辑”，而是“让现有操作系统兼容并运行一个真实 C 程序”；
- 如果改写成 Rust，很多本该暴露出来的兼容性问题会被绕开，例如：
  - C 运行时初始化；
  - `sbrk/brk` 风格的堆增长；
  - `lseek` / `read` / `write` 的使用方式；
  - C 程序对文件系统、用户缓冲区、内存布局的真实假设。

换句话说，保留 Doom 为 C 程序，才能真正检验这个操作系统能否支撑一个非教学玩具级别的用户程序。

代价：

- 必须额外提供裸机 RISC-V 的 C 编译链；
- 必须自己补一层 C 程序到 OS 系统调用之间的胶水代码；
- 必须处理比 Rust tutorial 用户程序复杂得多的兼容问题。

但这个代价正是题目要求的一部分，所以这是正确的方向。

### 2. 选择“通过 `build.rs` 集成 Doom 构建”，而不是单独手工编译

第二个关键选择是：Doom 应该怎样接入现有工程构建流程。

当时也有两个可行方案：

- 路径 A：手工在项目外先编好 Doom，再拷到文件系统镜像里；
- 路径 B：把 Doom 的构建集成进项目自己的 `build.rs`，让 `cargo build` / `cargo run` 直接产出可启动镜像。

我选择了路径 B。

原因：

- 题目明确要求“`cargo run` 后直接启动”；
- 如果依赖手工预编译，流程不稳定，验收时很容易出现“代码改了，但镜像里还是旧程序”的问题；
- 只有把 Doom 放进项目自身的 build pipeline，才能保证：
  - C 源码改动会触发重编译；
  - 用户程序镜像会自动重打包；
  - `cargo run` 路径和最终交付路径一致。

因此我把 Doom C 源码编译逻辑接进了 [build.rs](/Users/lorenzlorentz/tg-rcore-tutorial/tg-rcore-tutorial-t3l8-doom/build.rs)，并通过现有 easy-fs 打包流程把它放进文件系统镜像。

### 3. 选择“通过 `initproc -> exec("doom")` 自动启动”，而不是让内核直接跳进 Doom

题目要求游戏在 `cargo run` 后直接启动，但这并不意味着最合理的做法是把 Doom 硬塞进内核。

当时有两个思路：

- 路径 A：让内核在启动后直接加载或跳转到 Doom；
- 路径 B：继续保持操作系统抽象边界，由 `initproc` 像启动普通用户程序一样 `exec("doom")`。

我选择了路径 B。

原因：

- 这样才能验证“当前 OS 的进程加载、文件系统、ELF 装载、系统调用、地址空间切换”整条链路；
- 如果内核直接跳进 Doom，虽然表面上也能“跑起来”，但实际上绕开了最关键的一层：用户程序兼容性；
- 题目中还强调“妥善处理游戏程序的 exec”，这本身就是在提示启动路径应该走 `exec`。

因此我把 `initproc` 改成优先 `exec("doom")`，这既满足了“开机直进游戏”，也没有破坏用户态与内核态的边界。

### 4. 选择“补最小必要的 POSIX/类 Unix 能力”，而不是实现一个大而全兼容层

一开始我就知道 Doom 不会只用 tutorial 里那些最简单的系统调用。

这里存在两种工程风格：

- 路径 A：为了保险，一次性补很多类 POSIX 能力；
- 路径 B：基于真实运行报错，补 Doom 实际依赖的最小能力集合。

我选择了路径 B。

最后实际补到的能力主要有：

- `lseek`
- `sbrk/brk`
- 跨页用户缓冲区的 `read/write`
- 图形 framebuffer 查询与 present
- 键盘输入轮询

原因：

- 这是教学 OS，不适合无边界地往里堆兼容层；
- 每多补一块“猜测式兼容”，就多一块潜在回归风险；
- Doom 的需求虽然比教学程序复杂，但并没有要求完整 Unix。

这个选择的本质是：只为真实依赖负责，不为想象中的未来依赖负责。

### 5. 选择“修真实崩溃根因”，而不是在渲染层做保护性兜底

后续 Doom 曾经出现过“游戏已经运行一段时间后，突然在 UI/状态栏绘制阶段崩溃”的问题。

面对这种问题，通常有两条路：

- 路径 A：在渲染函数里加大量边界检查，尽量不让程序崩；
- 路径 B：把崩溃一路追到真正的数据损坏来源，再修根因。

我选择了路径 B。

原因：

- 渲染层拿到坏指针，往往只是“最后一个出事的地方”，不是第一个出错的地方；
- 如果只在渲染层兜底，虽然表面可能不崩，但数据已经坏了，游戏状态会越来越不可信；
- 这次任务要求的是“可游玩”，不是“靠无数 if 防住 crash 但内部状态全乱”。

最后确认的真实根因是：

- `st_stuff.c` 里某段代码把 `weaponowned[]` 这个按字节存储的布尔数组，错误地当成了 `int *` 传给 widget 状态；
- 这会在周围字节变化后读出垃圾索引；
- 最终导致状态栏图标 `patch*` 指针非法，渲染时崩溃。

因此最终修的是状态数据类型，而不是渲染层。

### 6. 选择“通过默认绑定把控制改成 WASD”，而不是把 `W/A/S/D` 伪装成方向键

你后来反馈 `WASD` 没反应，这是一个很典型的“控制策略放在哪一层”的问题。

这里也有两种办法：

- 路径 A：在端口层把 `W` 硬翻译成 `KEY_UPARROW`，把 `A` 翻成 `KEY_LEFTARROW`，等等；
- 路径 B：保持按键本身还是 `w/a/s/d`，但是修改 Doom 自己的默认按键绑定，让它们成为前进/后退/平移。

我选择了路径 B。

原因：

- 端口层负责“按下的是哪个物理/逻辑键”，不应该负责“这个键在游戏里代表什么动作”；
- Doom 自己已经有一套成熟的 key binding 机制；
- 如果端口层直接把 `W` 伪装成方向键，会污染菜单、输入框、聊天等其他功能的语义。

因此我最终改的是 [m_controls.c](/Users/lorenzlorentz/tg-rcore-tutorial/tg-rcore-tutorial-t3l8-doom/doomgeneric/doomgeneric/m_controls.c)，把默认绑定调整为：

- `W`：前进
- `S`：后退
- `A`：左平移
- `D`：右平移
- 左右方向键：继续承担转向

同时，菜单和 automap 默认控制也一并改成了 `WASD`，避免出现“游戏里是 WASD，但菜单里还是方向键”的割裂体验。

### 7. 选择“对启动期疑似误触的 `Esc` 做窄修正”，而不是粗暴改坏菜单逻辑

你反馈“开始游玩之后，游戏菜单一直显示着”。这个问题在我用 QEMU monitor 注入按键时没有稳定复现。

我验证到的事实是：

- `Esc` 本身可以正常打开菜单；
- 再按一次 `Esc` 可以正常关闭菜单；
- 正常启动后并不是每次都天然带着菜单层。

这说明 Doom 的菜单逻辑本身没有坏。

因此我没有选择以下危险做法：

- 每帧都强制 `menuactive = false`；
- 直接修改 `m_menu.c` 核心逻辑，硬压菜单状态；
- 把 `Esc` 彻底禁用。

我实际做的，是在端口层加了一个非常窄的修正：

- 如果在启动后的前两秒内；
- 还没有发生过真实用户键按下；
- 第一次收到的按键恰好是 `Esc`；
- 就把这一个 `Esc` 丢掉。

这样做的逻辑是：

- 这更像是宿主机/QEMU 焦点建立过程里带进来的异常 `Esc`；
- 它只处理一个非常具体的误触窗口；
- 不破坏正常菜单功能。

这是一个保守修正，不是大手术。

---

## 二、我遇到了什么困难，以及如何解决

### 困难 1：项目原本没有“C 用户程序进 easy-fs 并自动启动”的完整链路

这是最基础但也是最根本的问题。

具体表现：

- 原 tutorial 用户程序主要是 Rust；
- 没有现成机制把一个外部 C 应用编译成 RISC-V 用户态 ELF 后再装进 easy-fs；
- 也没有一套现成的 bare-metal C runtime 胶水直接给 Doom 用。

解决方法：

1. 在 `capps/doom/` 下补了 Doom 端口代码；
2. 用本机安装好的 `riscv-none-elf-gcc` 进行裸机编译；
3. 在 `build.rs` 中把 Doom 编译纳入整个项目构建流程；
4. 复用现有 easy-fs 打包逻辑，把 Doom 可执行文件打进 `fs.img`；
5. 修改 `initproc`，使它启动后优先 `exec("doom")`。

这样才真正形成了从源码到游戏启动的闭环。

### 困难 2：C 程序比 tutorial 里的简单程序更依赖真实的文件语义

Doom 启动过程中大量读取 WAD，读法并不是“打开一次顺序读到底”这么简单。

实际问题包括：

- 需要 `lseek`
- 需要知道文件大小
- 需要频繁从不同 offset 取数据块

而原 tutorial 环境里，这些需求并没有被充分覆盖。

解决方法：

- 在内核 syscall 层补 `lseek`
- 在文件句柄层补 `seek`
- 在 inode 层补 `size`
- 在 C syscall shim 中把 `_lseek_r` 接到系统调用

相关位置包括：

- [main.rs](/Users/lorenzlorentz/tg-rcore-tutorial/tg-rcore-tutorial-t3l8-doom/src/main.rs)
- [fs.rs](/Users/lorenzlorentz/tg-rcore-tutorial/tg-rcore-tutorial-t3l8-doom/src/fs.rs)
- [file.rs](/Users/lorenzlorentz/tg-rcore-tutorial/tg-rcore-tutorial-easy-fs/src/file.rs)
- [vfs.rs](/Users/lorenzlorentz/tg-rcore-tutorial/tg-rcore-tutorial-easy-fs/src/vfs.rs)

这部分解决之后，Doom 的 WAD 读取路径才算真正站稳。

### 困难 3：C 运行时需要动态堆，而原系统没有真正满足这件事

光能读文件还不够。Doom 作为一个完整 C 程序，还依赖运行时堆增长。

具体表现：

- newlib 相关路径会调用 `_sbrk_r`
- 如果 OS 不支持 `brk/sbrk`，程序可能在初始化内存区、缓存、运行时对象时失败

解决方法：

- 增加 `sbrk/brk` syscall
- 在 `Process` 中引入 `heap_bottom` 和 `program_brk`
- 提供 `change_program_brk()` 用来动态映射/回收用户堆页

这是 Doom 能稳定运行的必要条件之一。

### 困难 4：之前的 `read/write` 假设用户缓冲区不会跨页，这对 Doom 不成立

这个问题比 `lseek` 更隐蔽。

原来简单程序的 I/O 缓冲区往往很小，很多情况下不会跨页，所以“把一个用户指针翻译成一段连续 slice”这件事暂时没暴露问题。

但 Doom 会使用更大的 C 缓冲区，跨页就成为常态。

如果仍然沿用旧做法，会出现：

- 内核只翻译第一页；
- 后半段访问落到无效地址；
- 读写不完整或者直接崩。

解决方法：

- 在 syscall 层增加 `translate_user_buffer()`
- 按页切片，把一个逻辑连续用户缓冲区拆成多个 `&mut [u8]`
- 用 `UserBuffer` 把这些碎片重新交给文件系统/管道层

这是一个典型的“教学代码够用，但真实程序不够用”的问题。

### 困难 5：游戏已经启动和渲染，但会在稍后某个 UI 绘制点崩溃

这是整个调试过程中最难的一个问题。

难点在于：

- 它不是开机即崩；
- 不是加载 WAD 时崩；
- 不是 framebuffer 初始化时崩；
- 而是已经能看到游戏画面，甚至能运行一段时间后才崩。

这类问题最危险，因为它意味着：

- 问题可能不是“缺一个 syscall”；
- 而是“某处数据悄悄坏了，最后在渲染阶段炸出来”。

我的定位过程大致是：

1. 在 `V_DrawPatch()` 附近加临时日志，看崩之前到底在画什么；
2. 在 HUD 相关路径里加临时检查，验证 patch 指针是否异常；
3. 增强内核 trap 日志，记录：
   - `sepc`
   - `stval`
   - `ra`
   - `sp`
   - `a0/a1/a2`
4. 把 `ra` 对应回 Doom 的具体函数；
5. 发现崩溃点落在状态栏 widget 对 weapon 图标的处理；
6. 最终追到 `weaponowned[]` 被错误按 `int *` 使用。

最终修法：

- 新增 `static int armsowned[6]`
- 每 tick 从 `plyr->weaponowned[]` 同步一次到这个整型数组
- widget 只接收 `&armsowned[i]`

这是本次移植里最典型的一次“不能靠猜，只能靠寄存器 + 调用链 + 局部插桩一点点逼近”的问题。

### 困难 6：QEMU 中已经有画面，但截图、输入复现、问题确认都不能只靠肉眼

题目要求最终截图，而且很多问题单靠“我看了一眼窗口”不够可靠。

尤其是输入问题，如果只在图形窗口里手动试，很难保留严格证据链。

解决方法：

- 除了正常 `cargo run` 路径，还额外使用带 monitor socket 的 QEMU 启动脚本；
- 通过 QEMU monitor 做：
  - `screendump`
  - `sendkey`
- 把关键状态直接导出成 `.ppm/.png`
- 对“某个键到底有没有作用”做可重复验证

这样我才能比较严格地判断：

- 启动后菜单是不是天然开启；
- `Esc` 是否能正常开关菜单；
- `WASD` 是否只是你本机感觉无效，还是系统性没绑定。

### 困难 7：`WASD` 无反应并不代表输入层坏了，可能只是默认键位仍是原版 Doom

这是后来你反馈后出现的问题。

一开始这个现象容易让人怀疑：

- 键盘驱动没工作；
- `event.value` 被解释错了；
- release/press 丢失；
- DoomGeneric port 层没把按键正确传进去。

但进一步分析后发现，真正的问题更可能是：

- 我们端口层已经把 `w/a/s/d` 作为普通字母传入 Doom；
- 而 Doom 原版默认移动键并不是 `WASD`，而是方向键系。

也就是说：输入本身没坏，但默认控制策略不符合现代玩家预期。

解决方法：

- 改 Doom 默认 binding，而不是乱改底层按键编码；
- 把菜单与 automap 一并同步成 `WASD`。

这是“问题表现像驱动问题，但根因其实是默认配置问题”的典型案例。

### 困难 8：你反馈“菜单一直显示”，但这个问题在我用 monitor 注入输入时不能稳定重现

这是另一个比较棘手的问题。

我实际验证到的是：

- 游戏可以在无菜单状态下启动；
- `Esc` 可以正常打开菜单；
- 再按 `Esc` 可以关闭菜单；
- 所以 Doom 的菜单开关逻辑本身没坏。

这意味着：如果菜单在你实际游玩时“总挂着”，更可能是某个外部交互因素触发了一个额外 `Esc`，而不是 `m_menu.c` 本身坏了。

在不能稳定复现的前提下，直接去硬改 Doom 菜单逻辑风险很高。

所以我的解决策略是：

- 不动 Doom 菜单核心状态机；
- 在端口层做一个极窄的启动期 `Esc` 过滤；
- 同时补 `ack_interrupt()`，收紧输入队列处理。

这是一个典型的保守工程选择：

- 当根因无法 100% 复现时，先做低风险、可解释、范围很小的修正；
- 避免一把改穿核心逻辑。

---

## 三、我如何验证这些修改

为了避免“看起来能跑，实际上问题没解决”，我采用的是分层验证。

### 1. 构建验证

先用 `cargo build` 确认：

- Rust 内核能过；
- `build.rs` 能正确触发 Doom C 编译；
- easy-fs 镜像能重新打包；
- 新的用户程序 `doom` 被正确写入 `fs.img`。

### 2. 启动链路验证

再用 `cargo run` 确认：

- 默认 runner 会启动 QEMU；
- 内核启动后会进入 `initproc`；
- `initproc` 会优先 `exec("doom")`；
- Doom 会真正开始初始化，而不是掉回 shell。

### 3. 图形与截图验证

使用带 monitor socket 的 QEMU 路径验证：

- framebuffer 是否真的在刷新；
- 截图文件是否是有效游戏画面，而不是黑屏/启动页；
- 截图内容是否足以作为最终交付依据。

### 4. 输入与菜单验证

使用 QEMU monitor 的 `sendkey` 做定向实验，验证：

- `Esc` 是否可以正常开/关菜单；
- `W/S` 在菜单里是否能承担上下导航；
- 启动期第一下 `Esc` 是否被正确过滤；
- 游戏内控制修改后，`WASD` 的默认行为是否已经按预期改变。

这种验证方式比单纯“手感测试”更可靠，因为可重复、可对比、可截图。

---

## 四、本次开发里最关键的文件

如果后续要继续维护，这几个文件最重要：

- [build.rs](/Users/lorenzlorentz/tg-rcore-tutorial/tg-rcore-tutorial-t3l8-doom/build.rs)
  - 决定 Doom 如何被编译、打包进文件系统镜像。
- [tg-rcore-tutorial-user/src/bin/initproc.rs](/Users/lorenzlorentz/tg-rcore-tutorial/tg-rcore-tutorial-t3l8-doom/tg-rcore-tutorial-user/src/bin/initproc.rs)
  - 决定系统启动后如何自动进入 Doom。
- [capps/doom/doomgeneric_rcore.c](/Users/lorenzlorentz/tg-rcore-tutorial/tg-rcore-tutorial-t3l8-doom/capps/doom/doomgeneric_rcore.c)
  - DoomGeneric 和当前 OS 之间的图形/输入桥接层。
- [src/main.rs](/Users/lorenzlorentz/tg-rcore-tutorial/tg-rcore-tutorial-t3l8-doom/src/main.rs)
  - syscall 分发、平台 syscall、I/O、`sbrk`、`lseek` 等关键实现都在这里接起来。
- [src/process.rs](/Users/lorenzlorentz/tg-rcore-tutorial/tg-rcore-tutorial-t3l8-doom/src/process.rs)
  - 用户堆和 `program_brk` 管理。
- [src/fs.rs](/Users/lorenzlorentz/tg-rcore-tutorial/tg-rcore-tutorial-t3l8-doom/src/fs.rs)
  - 文件描述符层的 `seek` 等包装。
- [tg-rcore-tutorial-easy-fs/src/file.rs](/Users/lorenzlorentz/tg-rcore-tutorial/tg-rcore-tutorial-easy-fs/src/file.rs)
  - 文件句柄 `seek` 和跨页 `UserBuffer` 行为。
- [doomgeneric/doomgeneric/st_stuff.c](/Users/lorenzlorentz/tg-rcore-tutorial/tg-rcore-tutorial-t3l8-doom/doomgeneric/doomgeneric/st_stuff.c)
  - 那个最隐蔽的状态栏崩溃根因修复就在这里。
- [doomgeneric/doomgeneric/m_controls.c](/Users/lorenzlorentz/tg-rcore-tutorial/tg-rcore-tutorial-t3l8-doom/doomgeneric/doomgeneric/m_controls.c)
  - 默认控制策略，包括现在的 `WASD` 绑定。

---

## 五、当前结论

到目前为止，这次移植中最重要的工程问题都已经跨过去了：

- Doom 已作为真正的 C 用户程序接入当前 OS；
- `cargo run` 后可以直接进入 Doom；
- 文件系统、WAD 读取、堆增长、跨页 I/O、图形接口、键盘输入都已经接通；
- 运行期那个真正会导致崩溃的数据类型错误已经修掉；
- 针对你反馈的交互问题，也已经补了默认 `WASD` 绑定和启动期 `Esc` 防误触修正。

如果后续还要继续打磨，下一阶段最值得继续观察的是：

- 在你本机图形窗口焦点切换场景下，菜单误弹是否已经完全消失；
- `WASD` 在你实际游玩环境中是否已经和现在的默认绑定一致；
- 是否还需要补鼠标、声音、保存/加载等进一步体验项。
