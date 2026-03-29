#!/usr/bin/env python3
import argparse
import os
import pathlib
import re
import subprocess
import sys


SUMMARY_RE = re.compile(r"^\[t2l4-summary\]\s+(.*)$", re.MULTILINE)


def parse_summary(output: str) -> dict[str, str]:
    match = SUMMARY_RE.search(output)
    if not match:
        raise RuntimeError("missing [t2l4-summary] line")
    fields = {}
    for item in match.group(1).split():
        key, value = item.split("=", 1)
        fields[key] = value
    return fields


def format_ms(value_us: int) -> str:
    return f"{value_us / 1000:.3f}"


def format_throughput(value_milli: int) -> str:
    return f"{value_milli / 1000:.3f}"


def run_case(t2l4_dir: pathlib.Path, logs_dir: pathlib.Path, scheduler: str, scenario: str, trace: bool) -> dict[str, str]:
    env = os.environ.copy()
    env["CHAPTER"] = "9"
    env["T2L4_SCHED"] = scheduler
    env["T2L4_SCENARIO"] = scenario
    env["T2L4_TRACE"] = "1" if trace else "0"
    cmd = ["cargo", "run"]
    proc = subprocess.run(
        cmd,
        cwd=t2l4_dir,
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        encoding="utf-8",
        errors="replace",
        check=False,
    )
    output = proc.stdout
    log_path = logs_dir / f"{scenario}_{scheduler}.log"
    log_path.write_text(output, encoding="utf-8")
    if proc.returncode != 0:
        raise RuntimeError(f"{scenario}/{scheduler} failed, see {log_path}")
    summary = parse_summary(output)
    summary["log_path"] = str(log_path)
    return summary


def print_table(rows: list[dict[str, str]]) -> None:
    header = (
        "scenario",
        "scheduler",
        "tasks",
        "avg_wait_ms",
        "avg_turn_ms",
        "throughput/s",
        "p95_ms",
        "p99_ms",
        "starvation",
    )
    print(" | ".join(header))
    print("-|-|-|-|-|-|-|-|-")
    for row in rows:
        print(
            " | ".join(
                [
                    row["scenario"],
                    row["scheduler"],
                    row["tasks"],
                    format_ms(int(row["avg_wait_us"])),
                    format_ms(int(row["avg_turnaround_us"])),
                    format_throughput(int(row["throughput_milli_per_s"])),
                    format_ms(int(row["p95_latency_us"])),
                    format_ms(int(row["p99_latency_us"])),
                    row["starvation"],
                ]
            )
        )


def main() -> int:
    parser = argparse.ArgumentParser(description="Run the t2l4 scheduler experiment matrix.")
    parser.add_argument(
        "--scenarios",
        default="cpu,io,interactive,mixed",
        help="Comma-separated scenario list.",
    )
    parser.add_argument(
        "--schedulers",
        default="fcfs,sjf,rr,mlfq,cfs",
        help="Comma-separated scheduler list.",
    )
    parser.add_argument(
        "--trace",
        action="store_true",
        help="Keep kernel trace output enabled during the run.",
    )
    args = parser.parse_args()

    t2l4_dir = pathlib.Path(__file__).resolve().parent.parent
    logs_dir = t2l4_dir / ".logs" / "suite"
    logs_dir.mkdir(parents=True, exist_ok=True)

    scenarios = [item.strip() for item in args.scenarios.split(",") if item.strip()]
    schedulers = [item.strip() for item in args.schedulers.split(",") if item.strip()]

    rows: list[dict[str, str]] = []
    for scenario in scenarios:
        for scheduler in schedulers:
            print(f"[suite] running scenario={scenario} scheduler={scheduler}", file=sys.stderr)
            rows.append(run_case(t2l4_dir, logs_dir, scheduler, scenario, args.trace))

    print_table(rows)
    print(f"\nlogs: {logs_dir}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
