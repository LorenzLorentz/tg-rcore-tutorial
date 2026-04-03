# T4L45 Baseline Recovery Notes

## 2026-04-04

- 目标重新定义为 README 中描述的联合 baseline：单核、虚拟 `lab_tick()`、统一调度报告、同步实验语义齐全，但不包含真实 timer interrupt 和 SMP。
- 恢复策略选为“从当前 `t4l45` 拷出一个独立目录，再回退 `main/processor/process` 到单核骨架”，避免把当前实现直接回滚。
- 发现一个评测污染源：仓库根目录的 [`.cargo/config.toml`](/Users/lorenzlorentz/tg-rcore-tutorial/.cargo/config.toml) 会把 `kernel-alloc` 和 `sync` patch 到当前 SMP 版实现。
- 处理中间方案：
  - 最初曾创建临时的 `*-baseline` 组件副本，用来隔离评测污染源。
  - 在把原始无后缀组件恢复到旧语义之后，`t4l45-baseline` 已改回直接依赖无后缀组件。
- 当前待解决问题：
  - `t2l4` 单核骨架与当前 `sync` API 有偏移，尤其是 `condvar_signal/condvar_wait` 返回值和参数。
  - baseline 需要继续兼容当前用户态 `T4L45_TRACE_GET_CURRENT_HART` 请求，预计在基线里固定返回 `0`。

## 2026-04-04 继续

- baseline 当前状态：
  - `cargo build --offline` 已通过。
  - `mixed/rr` 已能稳定跑出 `[t4l45-sched-summary]`。
  - `scheduler` 矩阵 20 组已完整跑通。
  - `sync` 成功套件 6 组已完整跑通。
- 为了避免当前工作目录的 `target/fs.img` 锁冲突，额外创建了两个评测副本：
  - `tg-rcore-tutorial-t4l45-current-smp2-eval`
  - `tg-rcore-tutorial-t4l45-current-smp1-eval`
- `current-smp1-eval`：
  - `scheduler` 矩阵 20 组已完整跑通。
  - `sync` 成功套件 6 组已完整跑通。
- `current-smp2-eval`：
  - 完整 `scheduler` 矩阵在第一个 `cpu/fcfs` 就失败。
  - 失败现象不是单纯 timeout，而是 workload 执行到中途重新打印 boot banner，说明存在重启/异常跳转。
  - 代表性稳定 workload 已确认两项：
    - `mixed/rr`
    - `t4l45_smp_probe`
- `control` 套件里观察到一个值得单独记录的点：
  - `t2l5_spin_broken` 没有按脚本预期表现为 timeout，而是走到了大块内存申请后 OOM panic。
  - 这说明“坏实现确实失败”仍然成立，但失败模式已经从“挂住”变成了“资源耗尽后崩溃”。
- 额外评测补充：
  - baseline `t4l45_smp_probe` 明确只看到 `mask=0x1`，符合单核预期。
  - current-smp2 `t4l45_smp_probe` 看到 `harts_seen=2`、`migrated_threads=8`。
  - baseline 和 current-smp1 都补跑了 `t4l45_hybrid_pipeline/rr`，两边都通过。
- 最终综合报告已写入 `evaluation-report.md`。
- 后续整理：
  - `t4l45-baseline` 现已显式依赖无后缀旧组件：`tg-rcore-tutorial-kernel-alloc`、`tg-rcore-tutorial-sbi`、`wpj-tg-rcore-tutorial-sync`。
  - 临时创建的 `tg-rcore-tutorial-kernel-alloc-baseline` 与 `tg-rcore-tutorial-sync-baseline` 已不再保留。
