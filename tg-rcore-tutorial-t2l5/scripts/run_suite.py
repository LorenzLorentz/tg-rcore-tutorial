#!/usr/bin/env python3
import argparse
import os
import pathlib
import re
import subprocess
import sys


SUMMARY_RE = re.compile(r"^\[t2l5-summary\]\s+(.*)$", re.MULTILINE)

SUCCESS_CASES = [
    {"scenario": "t2l5_spin_ticket", "timeout": 180},
    {"scenario": "t2l5_mutex_stress", "timeout": 180},
    {"scenario": "t2l5_semaphore_pc", "timeout": 180},
    {"scenario": "t2l5_condvar_pc", "timeout": 180},
    {"scenario": "t2l5_rwlock_fair", "timeout": 180},
    {"scenario": "t2l5_phil_mutex", "timeout": 180},
]

CONTROL_CASES = [
    {
        "scenario": "t2l5_bug_deadlock_mutex",
        "timeout": 90,
        "expected": "marker",
        "marker": "[t2l5-bug] source=kernel class=exact kind=deadlock primitive=mutex",
    },
    {
        "scenario": "t2l5_spin_broken",
        "timeout": 90,
        "expected": "marker",
        "marker": "[t2l5-bug] source=user class=heuristic kind=stuck_spin primitive=spinlock",
    },
    {
        "scenario": "t2l5_mutex_stress",
        "timeout": 90,
        "fault_mode": "mutex_drop_wakeup",
        "expected": "marker",
        "marker": "[t2l5-bug] source=kernel class=heuristic kind=lost_wakeup primitive=mutex",
    },
    {
        "scenario": "t2l5_semaphore_pc",
        "timeout": 90,
        "fault_mode": "semaphore_drop_wakeup",
        "expected": "marker",
        "marker": "[t2l5-bug] source=kernel class=heuristic kind=lost_wakeup primitive=semaphore",
    },
    {
        "scenario": "t2l5_condvar_if_bug",
        "timeout": 90,
        "expected": "marker",
        "marker": "[t2l5-bug] source=user class=exact kind=condvar_if_misuse primitive=condvar",
    },
    {
        "scenario": "t2l5_rwlock_reader_pref",
        "timeout": 90,
        "expected": "marker",
        "marker": "[t2l5-bug] source=user class=statistical kind=starvation primitive=rwlock",
    },
]


def parse_summary(output: str) -> dict[str, str]:
    match = SUMMARY_RE.search(output)
    if not match:
        raise RuntimeError("missing [t2l5-summary] line")
    fields: dict[str, str] = {}
    for item in match.group(1).split():
        key, value = item.split("=", 1)
        fields[key] = value
    return fields


def format_ms(value_us: str) -> str:
    return f"{int(value_us) / 1000:.3f}"


def run_cargo(
    t2l5_dir: pathlib.Path,
    logs_dir: pathlib.Path,
    scenario: str,
    timeout: int,
    fault_mode: str | None,
) -> dict[str, object]:
    env = os.environ.copy()
    env["CHAPTER"] = "10"
    env["T2L5_SCENARIO"] = scenario
    if fault_mode:
        env["T2L5_FAULT_MODE"] = fault_mode
    else:
        env.pop("T2L5_FAULT_MODE", None)

    suffix = f"__{fault_mode}" if fault_mode else ""
    log_path = logs_dir / f"{scenario}{suffix}.log"
    cmd = ["cargo", "run"]
    try:
        proc = subprocess.run(
            cmd,
            cwd=t2l5_dir,
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


def warmup_build(t2l5_dir: pathlib.Path, logs_dir: pathlib.Path) -> None:
    env = os.environ.copy()
    env["CHAPTER"] = "10"
    env["T2L5_SCENARIO"] = "t2l5_spin_ticket"
    env.pop("T2L5_FAULT_MODE", None)
    log_path = logs_dir / "warmup_build.log"
    proc = subprocess.run(
        ["cargo", "build"],
        cwd=t2l5_dir,
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=600,
        check=False,
    )
    log_path.write_text(proc.stdout, encoding="utf-8")
    if proc.returncode != 0:
        raise RuntimeError(f"warmup build failed, see {log_path}")


def summarize_success(
    t2l5_dir: pathlib.Path,
    logs_dir: pathlib.Path,
    case: dict[str, object],
) -> dict[str, str]:
    result = run_cargo(
        t2l5_dir,
        logs_dir,
        case["scenario"],
        int(case["timeout"]),
        case.get("fault_mode"),
    )
    if result["status"] != "ok":
        raise RuntimeError(f"{case['scenario']} failed, see {result['log_path']}")
    summary = parse_summary(str(result["output"]))
    summary["scenario"] = str(case["scenario"])
    summary["log_path"] = str(result["log_path"])
    return summary


def run_control(
    t2l5_dir: pathlib.Path,
    logs_dir: pathlib.Path,
    case: dict[str, object],
) -> dict[str, str]:
    result = run_cargo(
        t2l5_dir,
        logs_dir,
        case["scenario"],
        int(case["timeout"]),
        case.get("fault_mode"),
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


def display_ops(row: dict[str, str]) -> str:
    if "ops" in row:
        return row["ops"]
    if "read_ops" in row and "write_ops" in row:
        return f"{row['read_ops']}/{row['write_ops']}"
    return "-"


def print_success_table(rows: list[dict[str, str]]) -> None:
    print("success case | ops | avg_wait_ms | max_wait_ms | avg_hold_ms | ctx_switches | blocked | wakeups | starvation")
    print("-|-|-|-|-|-|-|-|-")
    for row in rows:
        label = f"{row['primitive']}:{row['variant']}"
        print(
            " | ".join(
                [
                    label,
                    display_ops(row),
                    format_ms(row["avg_wait_us"]),
                    format_ms(row["max_wait_us"]),
                    format_ms(row["avg_hold_us"]),
                    row["ctx_switches"],
                    row["blocked"],
                    row["wakeups"],
                    row["starvation"],
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


def main() -> int:
    parser = argparse.ArgumentParser(description="Run the t2l5 synchronization experiment suite.")
    parser.add_argument(
        "--mode",
        choices=["all", "success", "control"],
        default="all",
        help="Run only success cases, only controls, or both.",
    )
    args = parser.parse_args()

    t2l5_dir = pathlib.Path(__file__).resolve().parent.parent
    logs_dir = t2l5_dir / ".logs" / "suite"
    logs_dir.mkdir(parents=True, exist_ok=True)
    warmup_build(t2l5_dir, logs_dir)

    success_rows: list[dict[str, str]] = []
    control_rows: list[dict[str, str]] = []

    if args.mode in {"all", "success"}:
        for case in SUCCESS_CASES:
            print(f"[suite] success scenario={case['scenario']}", file=sys.stderr)
            success_rows.append(summarize_success(t2l5_dir, logs_dir, case))

    if args.mode in {"all", "control"}:
        for case in CONTROL_CASES:
            label = case["scenario"]
            if case.get("fault_mode"):
                label = f"{label} fault={case['fault_mode']}"
            print(f"[suite] control {label}", file=sys.stderr)
            control_rows.append(run_control(t2l5_dir, logs_dir, case))

    if success_rows:
        print_success_table(success_rows)
    if control_rows:
        print_control_table(control_rows)
    print(f"\nlogs: {logs_dir}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
