#!/usr/bin/env python3
import argparse
import os
import pathlib
import re
import subprocess
import sys


SCHED_SUMMARY_RE = re.compile(r"^\[t4l45-sched-summary\]\s+(.*)$", re.MULTILINE)
SYNC_SUMMARY_RE = re.compile(r"^\[t2l5-summary\]\s+(.*)$", re.MULTILINE)
T4L45_SUMMARY_RE = re.compile(r"^\[t4l45-summary\]\s+(.*)$", re.MULTILINE)

SCHEDULER_SCENARIOS = ["cpu", "io", "interactive", "mixed"]
SCHEDULERS = ["fcfs", "sjf", "rr", "mlfq", "cfs"]

SYNC_SUCCESS_CASES = [
    {"scenario": "t2l5_spin_ticket", "timeout": 180},
    {"scenario": "t2l5_mutex_stress", "timeout": 180},
    {"scenario": "t2l5_semaphore_pc", "timeout": 180},
    {"scenario": "t2l5_condvar_pc", "timeout": 180},
    {"scenario": "t2l5_rwlock_fair", "timeout": 180},
    {"scenario": "t2l5_phil_mutex", "timeout": 180},
]

SYNC_CONTROL_CASES = [
    {"scenario": "t2l5_spin_broken", "timeout": 90, "expected": "timeout"},
    {
        "scenario": "t2l5_mutex_stress",
        "timeout": 90,
        "fault_mode": "mutex_drop_wakeup",
        "expected": "timeout",
    },
    {
        "scenario": "t2l5_semaphore_pc",
        "timeout": 90,
        "fault_mode": "semaphore_drop_wakeup",
        "expected": "timeout",
    },
    {
        "scenario": "t2l5_condvar_if_bug",
        "timeout": 90,
        "expected": "marker",
        "marker": "[t2l5-control] condvar if-bug failed_threads=",
    },
    {
        "scenario": "t2l5_rwlock_reader_pref",
        "timeout": 90,
        "expected": "marker",
        "marker": "reader-prefer rwlock starved writer as expected",
    },
]

ROBUST_SYNC_CASES = [
    {"scenario": "t2l5_mutex_stress", "timeout": 240},
    {"scenario": "t2l5_condvar_pc", "timeout": 240},
    {"scenario": "t2l5_rwlock_fair", "timeout": 240},
]

ROBUST_COMPLEX_CASES = [
    {
        "scenario": "t4l45_hybrid_pipeline",
        "timeout": 300,
        "schedulers": ["fcfs", "sjf", "rr", "cfs"],
    },
    {
        "scenario": "t4l45_semaphore_ring",
        "timeout": 240,
        "schedulers": ["fcfs", "sjf", "rr", "cfs"],
    },
]


def parse_fields(regex: re.Pattern[str], output: str, label: str) -> dict[str, str]:
    match = regex.search(output)
    if not match:
        raise RuntimeError(f"missing {label}")
    fields: dict[str, str] = {}
    for item in match.group(1).split():
        key, value = item.split("=", 1)
        fields[key] = value
    return fields


def format_ms(value_us: str | int) -> str:
    return f"{int(value_us) / 1000:.3f}"


def format_throughput(value: str | int) -> str:
    return f"{float(value):.3f}"


def display_ops(row: dict[str, str]) -> str:
    if "ops" in row:
        return row["ops"]
    if "read_ops" in row and "write_ops" in row:
        return f"{row['read_ops']}/{row['write_ops']}"
    return "-"


def run_cargo(
    t4l45_dir: pathlib.Path,
    logs_dir: pathlib.Path,
    scenario: str,
    scheduler: str,
    timeout: int,
    trace: bool = False,
    fault_mode: str | None = None,
) -> dict[str, object]:
    env = os.environ.copy()
    env["CHAPTER"] = "45"
    env["T4L45_SCENARIO"] = scenario
    env["T4L45_SCHED"] = scheduler
    env["T4L45_TRACE"] = "1" if trace else "0"
    if fault_mode:
        env["T4L45_FAULT_MODE"] = fault_mode
    else:
        env.pop("T4L45_FAULT_MODE", None)

    suffix = f"__{scheduler}"
    if trace:
        suffix += "__trace"
    if fault_mode:
        suffix += f"__{fault_mode}"
    log_path = logs_dir / f"{scenario}{suffix}.log"

    try:
        proc = subprocess.run(
            ["cargo", "run"],
            cwd=t4l45_dir,
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout,
            check=False,
        )
        output = proc.stdout
        log_path.write_text(output, encoding="utf-8")
        return {
            "status": "ok" if proc.returncode == 0 else "nonzero",
            "returncode": proc.returncode,
            "output": output,
            "log_path": str(log_path),
        }
    except subprocess.TimeoutExpired as exc:
        if isinstance(exc.stdout, bytes):
            output = exc.stdout.decode("utf-8", "replace")
        else:
            output = exc.stdout or ""
        log_path.write_text(output, encoding="utf-8")
        return {
            "status": "timeout",
            "returncode": None,
            "output": output,
            "log_path": str(log_path),
        }


def warmup_build(t4l45_dir: pathlib.Path, logs_dir: pathlib.Path) -> None:
    env = os.environ.copy()
    env["CHAPTER"] = "45"
    env["T4L45_SCENARIO"] = "mixed"
    env["T4L45_SCHED"] = "rr"
    env["T4L45_TRACE"] = "0"
    env.pop("T4L45_FAULT_MODE", None)

    log_path = logs_dir / "warmup_build.log"
    proc = subprocess.run(
        ["cargo", "build"],
        cwd=t4l45_dir,
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=900,
        check=False,
    )
    log_path.write_text(proc.stdout, encoding="utf-8")
    if proc.returncode != 0:
        raise RuntimeError(f"warmup build failed, see {log_path}")


def run_scheduler_case(
    t4l45_dir: pathlib.Path,
    logs_dir: pathlib.Path,
    scenario: str,
    scheduler: str,
    trace: bool,
) -> dict[str, str]:
    result = run_cargo(t4l45_dir, logs_dir, scenario, scheduler, 240, trace=trace)
    if result["status"] != "ok":
        raise RuntimeError(f"{scenario}/{scheduler} failed, see {result['log_path']}")
    summary = parse_fields(SCHED_SUMMARY_RE, str(result["output"]), "[t4l45-sched-summary]")
    summary["log_path"] = str(result["log_path"])
    return summary


def run_sync_case(
    t4l45_dir: pathlib.Path,
    logs_dir: pathlib.Path,
    case: dict[str, object],
    scheduler: str,
) -> dict[str, str]:
    result = run_cargo(
        t4l45_dir,
        logs_dir,
        str(case["scenario"]),
        scheduler,
        int(case["timeout"]),
    )
    if result["status"] != "ok":
        raise RuntimeError(f"{case['scenario']} failed, see {result['log_path']}")
    summary = parse_fields(SYNC_SUMMARY_RE, str(result["output"]), "[t2l5-summary]")
    sched = parse_fields(
        SCHED_SUMMARY_RE,
        str(result["output"]),
        "[t4l45-sched-summary]",
    )
    summary["scenario"] = str(case["scenario"])
    summary["scheduler"] = scheduler
    summary["sched_p95_latency_us"] = sched["p95_latency_us"]
    summary["sched_ctx_switches"] = sched["ctx_switches"]
    summary["log_path"] = str(result["log_path"])
    return summary


def run_control_case(
    t4l45_dir: pathlib.Path,
    logs_dir: pathlib.Path,
    case: dict[str, object],
) -> dict[str, str]:
    result = run_cargo(
        t4l45_dir,
        logs_dir,
        str(case["scenario"]),
        "rr",
        int(case["timeout"]),
        fault_mode=case.get("fault_mode"),
    )
    output = str(result["output"])
    observed = str(result["status"])
    expected = str(case["expected"])
    if expected == "marker":
        marker = str(case["marker"])
        if marker not in output:
            raise RuntimeError(
                f"{case['scenario']} missing marker {marker!r}, see {result['log_path']}"
            )
        observed = "marker"
    elif observed != expected:
        raise RuntimeError(
            f"{case['scenario']} expected {expected} but observed {observed}, see {result['log_path']}"
        )
    return {
        "scenario": str(case["scenario"]),
        "fault_mode": str(case.get("fault_mode", "")),
        "expected": expected,
        "observed": observed,
        "log_path": str(result["log_path"]),
    }


def run_complex_case(
    t4l45_dir: pathlib.Path,
    logs_dir: pathlib.Path,
    case: dict[str, object],
    scheduler: str,
) -> dict[str, str]:
    result = run_cargo(
        t4l45_dir,
        logs_dir,
        str(case["scenario"]),
        scheduler,
        int(case["timeout"]),
    )
    if result["status"] != "ok":
        raise RuntimeError(f"{case['scenario']} failed, see {result['log_path']}")
    summary = parse_fields(T4L45_SUMMARY_RE, str(result["output"]), "[t4l45-summary]")
    sched = parse_fields(
        SCHED_SUMMARY_RE,
        str(result["output"]),
        "[t4l45-sched-summary]",
    )
    summary["scenario"] = str(case["scenario"])
    summary["scheduler"] = scheduler
    summary["sched_p95_latency_us"] = sched["p95_latency_us"]
    summary["sched_ctx_switches"] = sched["ctx_switches"]
    summary["log_path"] = str(result["log_path"])
    return summary


def print_scheduler_table(rows: list[dict[str, str]]) -> None:
    print("scenario | scheduler | tasks | avg_wait_ms | avg_turn_ms | throughput/s | p95_ms | p99_ms | starvation | ctx_switches")
    print("-|-|-|-|-|-|-|-|-|-")
    for row in rows:
        print(
            " | ".join(
                [
                    row["scenario"],
                    row["scheduler"],
                    row["tasks"],
                    format_ms(row["avg_wait_us"]),
                    format_ms(row["avg_turnaround_us"]),
                    format_throughput(int(row["throughput_milli_per_s"]) / 1000),
                    format_ms(row["p95_latency_us"]),
                    format_ms(row["p99_latency_us"]),
                    row["starvation"],
                    row["ctx_switches"],
                ]
            )
        )


def print_sync_table(rows: list[dict[str, str]], title: str) -> None:
    print(f"\n{title}")
    print("case | scheduler | ops | avg_wait_ms | max_wait_ms | avg_hold_ms | ctx_switches | blocked | wakeups | starvation | sched_p95_ms")
    print("-|-|-|-|-|-|-|-|-|-|-")
    for row in rows:
        label = f"{row['primitive']}:{row['variant']}"
        print(
            " | ".join(
                [
                    label,
                    row["scheduler"],
                    display_ops(row),
                    format_ms(row["avg_wait_us"]),
                    format_ms(row["max_wait_us"]),
                    format_ms(row["avg_hold_us"]),
                    row["ctx_switches"],
                    row["blocked"],
                    row["wakeups"],
                    row["starvation"],
                    format_ms(row["sched_p95_latency_us"]),
                ]
            )
        )


def print_control_table(rows: list[dict[str, str]]) -> None:
    print("\ncontrol case | fault_mode | expected | observed")
    print("-|-|-|-")
    for row in rows:
        print(
            " | ".join(
                [
                    row["scenario"],
                    row["fault_mode"] or "-",
                    row["expected"],
                    row["observed"],
                ]
            )
        )


def print_complex_table(rows: list[dict[str, str]]) -> None:
    print("\ncomplex case | scheduler | ops | throughput/s | avg_wait_ms | max_wait_ms | avg_hold_ms | ctx_switches | blocked | wakeups | starvation | sched_p95_ms")
    print("-|-|-|-|-|-|-|-|-|-|-|-")
    for row in rows:
        print(
            " | ".join(
                [
                    row["case"],
                    row["scheduler"],
                    row["ops"],
                    row["throughput_ops_per_sec"],
                    format_ms(row["avg_wait_us"]),
                    format_ms(row["max_wait_us"]),
                    format_ms(row["avg_hold_us"]),
                    row["ctx_switches"],
                    row["blocked"],
                    row["wakeups"],
                    row["starvation"],
                    format_ms(row["sched_p95_latency_us"]),
                ]
            )
        )


def main() -> int:
    parser = argparse.ArgumentParser(description="Run the merged t4l45 scheduler + synchronization suite.")
    parser.add_argument(
        "--mode",
        choices=["all", "scheduler", "sync", "control", "robust"],
        default="all",
        help="Choose which part of the merged suite to run.",
    )
    parser.add_argument(
        "--trace",
        action="store_true",
        help="Keep scheduler trace output enabled during scheduler matrix runs.",
    )
    args = parser.parse_args()

    t4l45_dir = pathlib.Path(__file__).resolve().parent.parent
    logs_dir = t4l45_dir / ".logs" / "suite"
    logs_dir.mkdir(parents=True, exist_ok=True)
    warmup_build(t4l45_dir, logs_dir)

    scheduler_rows: list[dict[str, str]] = []
    sync_rows: list[dict[str, str]] = []
    control_rows: list[dict[str, str]] = []
    robust_sync_rows: list[dict[str, str]] = []
    complex_rows: list[dict[str, str]] = []

    if args.mode in {"all", "scheduler"}:
        for scenario in SCHEDULER_SCENARIOS:
            for scheduler in SCHEDULERS:
                print(
                    f"[suite] scheduler scenario={scenario} scheduler={scheduler}",
                    file=sys.stderr,
                )
                scheduler_rows.append(
                    run_scheduler_case(
                        t4l45_dir,
                        logs_dir,
                        scenario,
                        scheduler,
                        args.trace,
                    )
                )

    if args.mode in {"all", "sync"}:
        for case in SYNC_SUCCESS_CASES:
            print(f"[suite] sync scenario={case['scenario']} scheduler=rr", file=sys.stderr)
            sync_rows.append(run_sync_case(t4l45_dir, logs_dir, case, "rr"))

    if args.mode in {"all", "control"}:
        for case in SYNC_CONTROL_CASES:
            label = str(case["scenario"])
            if case.get("fault_mode"):
                label = f"{label} fault={case['fault_mode']}"
            print(f"[suite] control {label}", file=sys.stderr)
            control_rows.append(run_control_case(t4l45_dir, logs_dir, case))

    if args.mode in {"all", "robust"}:
        for case in ROBUST_SYNC_CASES:
            for scheduler in SCHEDULERS:
                print(
                    f"[suite] robust-sync scenario={case['scenario']} scheduler={scheduler}",
                    file=sys.stderr,
                )
                robust_sync_rows.append(run_sync_case(t4l45_dir, logs_dir, case, scheduler))
        for case in ROBUST_COMPLEX_CASES:
            schedulers = list(case.get("schedulers", SCHEDULERS))
            for scheduler in schedulers:
                print(
                    f"[suite] complex scenario={case['scenario']} scheduler={scheduler}",
                    file=sys.stderr,
                )
                complex_rows.append(run_complex_case(t4l45_dir, logs_dir, case, scheduler))

    if scheduler_rows:
        print_scheduler_table(scheduler_rows)
    if sync_rows:
        print_sync_table(sync_rows, "sync baseline")
    if control_rows:
        print_control_table(control_rows)
    if robust_sync_rows:
        print_sync_table(robust_sync_rows, "robust cross-scheduler sync")
    if complex_rows:
        print_complex_table(complex_rows)
    print(f"\nlogs: {logs_dir}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
