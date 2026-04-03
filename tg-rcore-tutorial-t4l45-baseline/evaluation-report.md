# T4L45 Baseline Recovery Evaluation

> Superseded by `../tg-rcore-tutorial-t4l45/evaluation-report.md` and `../../docs/report_task4_eval_seq.json`.
> This file records the earlier baseline-recovery-focused evaluation and should not be used as the final authoritative report.

## 1. Evaluated Artifacts

### 1.1 Recovered baseline

- Directory: `tg-rcore-tutorial-t4l45-baseline`
- Definition: the single-core merged baseline described in `t4l45/README`
  - single hart
  - virtual scheduler tick driven by `lab_tick()`
  - unified scheduler report
  - synchronization experiment semantics retained
  - no real timer-interrupt-driven preemption
  - no SMP scheduling

### 1.2 Current implementation, single-core eval copy

- Directory: `tg-rcore-tutorial-t4l45-current-smp1-eval`
- Source code: same as current `tg-rcore-tutorial-t4l45`
- Only change: runner forced back to single hart
- Purpose: isolate the cost/behavior change of the current implementation itself, without mixing in SMP effects

### 1.3 Current implementation, dual-core eval copy

- Directory: `tg-rcore-tutorial-t4l45-current-smp2-eval`
- Source code: same as current `tg-rcore-tutorial-t4l45`
- Runner: original 2-hart configuration
- Purpose: evaluate actual multicore behavior and stability

### 1.4 Why evaluation copies were created

- The live work directory already had `target/fs.img` users and historical logs.
- Separate copies avoided file-lock interference and kept the user’s working tree untouched.

## 2. Recovery Credibility and Limits

### 2.1 What was restored exactly

- `main.rs`, `processor.rs`, and `process.rs` were restored onto a single-core `t2l4` skeleton and then reconnected to the current `T4L45_*` user-facing trace/sync interface.
- The final `t4l45-baseline` now depends on the restored non-suffixed legacy components:
  - `tg-rcore-tutorial-kernel-alloc`
  - `tg-rcore-tutorial-sbi`
  - `wpj-tg-rcore-tutorial-sync`

### 2.2 What was reconstructed, not byte-identical

- During recovery, I temporarily created isolated `*-baseline` component copies to avoid pollution from the SMP-modified components.
- After the legacy non-suffixed components were restored, `t4l45-baseline` was switched back to those restored components and the temporary copies were made unnecessary.

### 2.3 Consequence

- Positive-path single-core benchmarking is credible.
- Negative-control parity still needs a little caution, because the baseline was reconstructed from available local history and compatible restored sources, not recovered from an archived original `t4l45-baseline` commit.

## 3. Test Method

- All commands were run with `CARGO_NET_OFFLINE=true` and `cargo ... --offline`.
- Scheduler matrix:
  - `python3 scripts/run_suite.py --mode scheduler`
  - run on `baseline` and `current-smp1`
  - 20 cases each: `4 scenarios x 5 schedulers`
- Sync success suite:
  - `python3 scripts/run_suite.py --mode sync`
  - run on `baseline` and `current-smp1`
  - 6 cases each
- Control suite:
  - fully run on `current-smp1`
  - attempted on `baseline`, but it diverged on the first case
- Extended complex workload:
  - `T4L45_SCENARIO=t4l45_hybrid_pipeline T4L45_SCHED=rr cargo run`
  - run on `baseline` and `current-smp1`
- Multicore-specific checks:
  - `T4L45_SCENARIO=t4l45_smp_probe T4L45_SCHED=rr cargo run`
  - `T4L45_SCENARIO=mixed T4L45_SCHED=rr cargo run`
  - attempted full scheduler matrix on `current-smp2`
  - attempted `t2l5_mutex_stress` on `current-smp2`

Raw logs are under each artifact’s `.logs/suite/` directory.

## 4. Top-Level Conclusions

1. The recovered baseline is usable for real benchmarking.
   - It builds.
   - It runs the full single-core scheduler matrix.
   - It runs the full single-core sync success suite.

2. The current implementation in single-core mode remains functionally solid, but generally slower than the recovered baseline.
   - The most important clue is that the scheduler matrix has the same `ctx_switches` counts case-by-case, while elapsed-time-derived metrics are usually worse.
   - That points to extra per-switch/per-trap overhead, not a different scheduling decision pattern.

3. The current implementation in dual-core mode is partially successful.
   - It clearly demonstrates 2-hart execution and cross-hart thread migration.
   - On `mixed/rr`, it improves waiting time and tail latency relative to `current-smp1`.
   - But it is not yet stable enough for a full scheduler matrix.

4. The strongest current risk is SMP stability, not basic single-core correctness.
   - `current-smp2` rebooted during `cpu/fcfs`.
   - `current-smp2` also failed to finish a representative sync stress case within the observation window.

## 5. Scheduler Matrix: Recovered Baseline

| scenario | scheduler | tasks | avg_wait_ms | avg_turn_ms | throughput/s | p95_ms | p99_ms | starvation | ctx_switches |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|
| cpu | fcfs | 6 | 601.036 | 1083.571 | 2.070 | 0.000 | 0.000 | 12 | 24 |
| cpu | sjf | 6 | 4.474 | 445.627 | 2.264 | 0.000 | 0.000 | 4 | 54 |
| cpu | rr | 6 | 862.388 | 1309.166 | 2.235 | 0.000 | 0.000 | 23 | 39 |
| cpu | mlfq | 6 | 825.259 | 1255.898 | 2.319 | 0.000 | 0.000 | 13 | 34 |
| cpu | cfs | 6 | 4.236 | 430.431 | 2.343 | 0.000 | 0.000 | 4 | 54 |
| io | fcfs | 6 | 3.963 | 1287.640 | 2.254 | 0.487 | 1.667 | 0 | 92 |
| io | sjf | 6 | 12.471 | 1260.428 | 2.314 | 19.087 | 19.087 | 4 | 156 |
| io | rr | 6 | 4.017 | 1369.356 | 2.148 | 0.488 | 1.651 | 0 | 92 |
| io | mlfq | 6 | 7.007 | 1301.068 | 2.242 | 0.050 | 0.152 | 0 | 125 |
| io | cfs | 6 | 6.709 | 1263.593 | 2.310 | 0.021 | 0.144 | 1 | 156 |
| interactive | fcfs | 6 | 4.389 | 1309.532 | 2.239 | 0.181 | 1.827 | 0 | 140 |
| interactive | sjf | 6 | 19.666 | 1282.805 | 2.290 | 21.688 | 21.688 | 8 | 252 |
| interactive | rr | 6 | 4.543 | 1422.969 | 2.068 | 0.513 | 1.604 | 0 | 140 |
| interactive | mlfq | 6 | 8.579 | 1325.400 | 2.211 | 0.043 | 0.155 | 0 | 197 |
| interactive | cfs | 6 | 8.081 | 1264.050 | 2.311 | 0.023 | 0.136 | 1 | 252 |
| mixed | fcfs | 7 | 92.222 | 1135.638 | 2.080 | 0.190 | 0.277 | 6 | 91 |
| mixed | sjf | 7 | 9.191 | 1105.782 | 1.990 | 12.506 | 12.506 | 5 | 165 |
| mixed | rr | 7 | 292.094 | 1345.862 | 2.062 | 0.201 | 0.263 | 13 | 97 |
| mixed | mlfq | 7 | 690.323 | 1757.516 | 2.055 | 0.036 | 0.146 | 5 | 128 |
| mixed | cfs | 7 | 6.069 | 1114.726 | 1.978 | 0.026 | 0.147 | 3 | 165 |

## 6. Scheduler Matrix: Current Implementation in Single-Core Mode

| scenario | scheduler | tasks | avg_wait_ms | avg_turn_ms | throughput/s | p95_ms | p99_ms | starvation | ctx_switches |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|
| cpu | fcfs | 6 | 695.984 | 1319.513 | 1.602 | 0.000 | 0.000 | 13 | 24 |
| cpu | sjf | 6 | 5.816 | 650.107 | 1.550 | 0.000 | 0.000 | 5 | 54 |
| cpu | rr | 6 | 1000.600 | 1580.088 | 1.723 | 0.000 | 0.000 | 24 | 39 |
| cpu | mlfq | 6 | 1003.115 | 1637.781 | 1.574 | 0.000 | 0.000 | 14 | 34 |
| cpu | cfs | 6 | 5.609 | 600.225 | 1.680 | 0.000 | 0.000 | 5 | 54 |
| io | fcfs | 6 | 5.061 | 1576.898 | 1.718 | 0.564 | 1.804 | 1 | 92 |
| io | sjf | 6 | 16.698 | 1705.688 | 1.645 | 6.356 | 23.460 | 6 | 156 |
| io | rr | 6 | 4.915 | 1557.380 | 1.759 | 0.448 | 1.776 | 1 | 92 |
| io | mlfq | 6 | 9.182 | 1561.340 | 1.737 | 0.084 | 0.246 | 0 | 125 |
| io | cfs | 6 | 9.619 | 1692.861 | 1.619 | 0.051 | 0.250 | 1 | 156 |
| interactive | fcfs | 6 | 5.622 | 1610.045 | 1.704 | 0.280 | 1.757 | 1 | 140 |
| interactive | sjf | 6 | 23.173 | 1621.124 | 1.703 | 25.663 | 30.634 | 6 | 252 |
| interactive | rr | 6 | 5.544 | 1544.848 | 1.773 | 0.263 | 1.988 | 1 | 140 |
| interactive | mlfq | 6 | 11.216 | 1538.207 | 1.785 | 0.050 | 0.237 | 0 | 197 |
| interactive | cfs | 6 | 11.768 | 1612.672 | 1.723 | 0.040 | 0.260 | 1 | 252 |
| mixed | fcfs | 7 | 108.656 | 1382.046 | 1.627 | 0.276 | 0.377 | 6 | 91 |
| mixed | sjf | 7 | 11.394 | 1278.279 | 1.644 | 14.484 | 14.484 | 5 | 165 |
| mixed | rr | 7 | 353.812 | 1635.327 | 1.631 | 0.335 | 0.399 | 16 | 97 |
| mixed | mlfq | 7 | 703.287 | 1795.771 | 1.975 | 0.054 | 0.223 | 6 | 128 |
| mixed | cfs | 7 | 7.557 | 1132.271 | 1.913 | 0.038 | 0.211 | 3 | 165 |

## 7. Single-Core Scheduler A/B Findings

### 7.1 Strongest pattern

- For every corresponding scheduler/scenario pair, `ctx_switches` stayed identical between `baseline` and `current-smp1`.
- But throughput and turnaround usually regressed.
- This strongly suggests the current implementation pays extra cost around each switch/tick/trap, instead of merely making different scheduling choices.

### 7.2 Representative `rr` comparison

| scenario | baseline wait_ms | current-smp1 wait_ms | delta | baseline throughput/s | current-smp1 throughput/s | delta |
|---|---:|---:|---:|---:|---:|---:|
| cpu | 862.388 | 1000.600 | +16.0% | 2.235 | 1.723 | -22.9% |
| io | 4.017 | 4.915 | +22.4% | 2.148 | 1.759 | -18.1% |
| interactive | 4.543 | 5.544 | +22.0% | 2.068 | 1.773 | -14.3% |
| mixed | 292.094 | 353.812 | +21.1% | 2.062 | 1.631 | -20.9% |

### 7.3 Mixed workload observation

- `mixed/rr` is the most representative single summary:
  - baseline:
    - `avg_wait_ms = 292.094`
    - `avg_turn_ms = 1345.862`
    - `throughput/s = 2.062`
    - `p95_ms = 0.201`
    - `starvation = 13`
  - current-smp1:
    - `avg_wait_ms = 353.812`
    - `avg_turn_ms = 1635.327`
    - `throughput/s = 1.631`
    - `p95_ms = 0.335`
    - `starvation = 16`

Interpretation:

- In single-core mode, the current implementation is clearly not a free upgrade over the recovered baseline.
- The cost is most visible in throughput and turnaround, while the scheduling structure itself remains similar.

## 8. Sync Success Suite: Recovered Baseline

| case | scheduler | ops | avg_wait_ms | max_wait_ms | avg_hold_ms | ctx_switches | blocked | wakeups | starvation | sched_p95_ms |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| spinlock:ticket | rr | 960 | 16.050 | 2439.806 | 2.396 | 15635 | 0 | 0 | 1 | 0.000 |
| mutex:fifo_blocking | rr | 960 | 15.054 | 2919.475 | 1.526 | 7753 | 959 | 959 | 2 | 0.141 |
| semaphore:producer_consumer | rr | 240 | 25.788 | 2416.315 | 3.788 | 2415 | 359 | 359 | 1 | 0.357 |
| condvar:producer_consumer | rr | 61 | 10.796 | 11.205 | 3.966 | 6079 | 66 | 126 | 0 | 0.116 |
| rwlock:fair | rr | 600/72 | 9.895 | 29.338 | 0.816 | 8485 | 1060 | 1060 | 0 | 0.459 |
| mutex:philosophers | rr | 20 | 136.623 | 1451.138 | 20.092 | 4704 | 18 | 18 | 0 | 0.572 |

## 9. Sync Success Suite: Current Implementation in Single-Core Mode

| case | scheduler | ops | avg_wait_ms | max_wait_ms | avg_hold_ms | ctx_switches | blocked | wakeups | starvation | sched_p95_ms |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| spinlock:ticket | rr | 960 | 18.919 | 2301.128 | 2.605 | 15635 | 0 | 0 | 1 | 0.000 |
| mutex:fifo_blocking | rr | 960 | 17.300 | 2890.194 | 1.739 | 7753 | 959 | 959 | 2 | 0.256 |
| semaphore:producer_consumer | rr | 240 | 27.047 | 2412.804 | 3.923 | 2415 | 359 | 359 | 1 | 0.444 |
| condvar:producer_consumer | rr | 61 | 11.210 | 12.366 | 3.889 | 4147 | 66 | 66 | 0 | 0.280 |
| rwlock:fair | rr | 600/72 | 10.834 | 12.767 | 0.862 | 8483 | 1060 | 1060 | 0 | 0.495 |
| mutex:philosophers | rr | 20 | 121.959 | 1373.028 | 20.763 | 2364 | 21 | 21 | 0 | 0.500 |

## 10. Sync A/B Findings

1. `baseline` and `current-smp1` both pass the sync success suite.

2. The current single-core implementation is again usually slower.
   - `mutex:fifo_blocking`
     - `avg_wait_ms`: `15.054 -> 17.300` (`+14.9%`)
     - `sched_p95_ms`: `0.141 -> 0.256` (`+81.6%`)
   - `semaphore:producer_consumer`
     - `avg_wait_ms`: `25.788 -> 27.047` (`+4.9%`)
   - `spinlock:ticket`
     - `avg_wait_ms`: `16.050 -> 18.919` (`+17.9%`)

3. The `condvar` wakeup count is not cleanly comparable.
   - `baseline` reported `wakeups=126`
   - `current-smp1` reported `wakeups=66`
   - Because the baseline sync package was reconstructed rather than restored byte-for-byte, this metric should not be over-interpreted.

## 11. Extended Complex Workload

### 11.1 `t4l45_hybrid_pipeline` under `rr`

#### baseline

- `[t4l45-summary]`
  - `ops=64`
  - `contention=127`
  - `avg_wait_us=43600`
  - `max_wait_us=1978064`
  - `avg_hold_us=6704`
  - `max_hold_us=814018`
  - `elapsed_us=2655537`
  - `throughput_ops_per_sec=24`
  - `ctx_switches=1595`
  - `blocked=127`
  - `wakeups=127`
  - `starvation=0`
- `[t4l45-sched-summary]`
  - `avg_wait_us=771694`
  - `avg_turnaround_us=1938818`
  - `throughput_milli_per_s=1393`
  - `p95_latency_us=140`
  - `p99_latency_us=2228`
  - `starvation=19`
  - `ctx_switches=1606`

#### current-smp1

- `[t4l45-summary]`
  - `ops=64`
  - `contention=127`
  - `avg_wait_us=43422`
  - `max_wait_us=1891361`
  - `avg_hold_us=6387`
  - `max_hold_us=753006`
  - `elapsed_us=2596814`
  - `throughput_ops_per_sec=24`
  - `ctx_switches=1595`
  - `blocked=127`
  - `wakeups=127`
  - `starvation=0`
- `[t4l45-sched-summary]`
  - `avg_wait_us=760560`
  - `avg_turnaround_us=1915189`
  - `throughput_milli_per_s=1448`
  - `p95_latency_us=392`
  - `p99_latency_us=2139`
  - `starvation=24`
  - `ctx_switches=1606`

### 11.2 Interpretation

- In this more integrated workload, the current single-core implementation is not uniformly worse.
- End-to-end throughput stayed the same at the workload level (`24 ops/s`).
- Scheduler tail latency became worse (`p95 140us -> 392us`), while some hold/wait aggregates improved slightly.
- This is why the evaluation cannot be reduced to one slogan like “everything got slower”.

## 12. Multicore Evaluation

### 12.1 Baseline `smp_probe`

- The recovered baseline correctly behaved as a single-core system.
- User-space observation:
  - `expected at least 2 harts, saw mask=0x1`
- Kernel still emitted:
  - `tasks=9`
  - `avg_wait_us=4105502`
  - `avg_turnaround_us=5106002`
  - `throughput_milli_per_s=997`
  - `starvation=1019`
  - `ctx_switches=1219`

### 12.2 Current `smp_probe` on 2 harts

- `[t4l45-summary]`
  - `case=smp_probe`
  - `threads=8`
  - `samples=96`
  - `harts_seen=2`
  - `migrated_threads=8`
  - `mask=0x3`
- `[t4l45-sched-summary]`
  - `tasks=9`
  - `avg_wait_us=2524162`
  - `avg_turnaround_us=3681640`
  - `throughput_milli_per_s=1401`
  - `starvation=924`
  - `ctx_switches=1251`

This is strong evidence that multicore bring-up and cross-hart scheduling are real, not cosmetic.

### 12.3 Current `mixed/rr`: single-core vs dual-core

| artifact | avg_wait_ms | avg_turn_ms | throughput/s | p95_ms | p99_ms | starvation | ctx_switches |
|---|---:|---:|---:|---:|---:|---:|---:|
| current-smp1 | 353.812 | 1635.327 | 1.631 | 0.335 | 0.399 | 16 | 97 |
| current-smp2 | 237.658 | 1813.703 | 1.814 | 0.052 | 0.146 | 3 | 97 |

Changes from `current-smp1 -> current-smp2`:

- `avg_wait_ms`: `-32.8%`
- `avg_turn_ms`: `+10.9%`
- `throughput/s`: `+11.2%`
- `p95_ms`: `-84.5%`
- `p99_ms`: `-63.4%`
- `starvation`: `-81.3%`

Interpretation:

- Dual-core mode materially improves responsiveness on the representative mixed workload.
- Average turnaround did not improve together with wait time and tail latency, so the multicore result is not yet a clean across-the-board speedup story.

### 12.4 Current `smp2` stability boundary

Attempted but not accepted as stable pass:

1. Full scheduler matrix on `current-smp2`
   - failed on the first case: `cpu/fcfs`
   - log showed the boot banner printed a second time after some user workers had already completed
   - this indicates reboot/abnormal control flow, not a mere slowdown

2. `t2l5_mutex_stress` on `current-smp2`
   - manual run did not emit a final summary even after a long observation window
   - I do not count it as a pass

Therefore:

- `current-smp2` is good enough to claim “multicore support exists and is observable”
- `current-smp2` is not yet good enough to claim “full scheduler/sync matrix is stable under SMP”

## 13. Control Suite

### 13.1 Current implementation in single-core mode

`current-smp1` passed the full control suite:

| case | fault_mode | expected | observed |
|---|---|---|---|
| t2l5_spin_broken | - | timeout | timeout |
| t2l5_mutex_stress | mutex_drop_wakeup | timeout | timeout |
| t2l5_semaphore_pc | semaphore_drop_wakeup | timeout | timeout |
| t2l5_condvar_if_bug | - | marker | marker |
| t2l5_rwlock_reader_pref | - | marker | marker |

### 13.2 Recovered baseline

The recovered baseline diverged on the first control case:

- `t2l5_spin_broken`
  - script expected `timeout`
  - observed behavior: OOM panic
  - log ended with:
    - `memory allocation of 33554432 bytes failed`

Interpretation:

- The bad program still failed, so the broad semantic point remains valid.
- But the failure mode is not identical to the reference control expectation.
- That is consistent with the fact that the baseline sync package had to be reconstructed rather than restored byte-for-byte.

## 14. Final Verdict

### 14.1 On baseline recovery

- Yes, a practical `t4l45-baseline` has been recovered.
- It is strong enough for positive-path performance comparison and scheduler/sync regression evaluation.
- It should not be treated as a perfect forensic reconstruction for every negative-control corner case.

### 14.2 On the current implementation

- Single-core:
  - functionally solid
  - full scheduler matrix passes
  - full sync success suite passes
  - full control suite passes
  - usually slower than baseline

- Dual-core:
  - real multicore execution is confirmed
  - mixed-workload responsiveness improves substantially
  - full SMP stability is not there yet

### 14.3 Most defensible statement

The current work has crossed the line from “single-core virtual baseline” to “a partially working interrupt/SMP kernel”: multicore execution and migration are real, and mixed-workload responsiveness improves on 2 harts, but the implementation still needs SMP stabilization before it can replace the recovered baseline as the only trustworthy evaluation target for the full suite.
