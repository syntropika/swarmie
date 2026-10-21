#!/usr/bin/env python3
"""Internal runtime diagnostics. Raw outputs must live outside the checkout."""

import argparse
import json
import os
from pathlib import Path
import platform
import shutil
import statistics
import subprocess
import sys

from common import (DiagnosticError, ROOT, SOURCE_IDENTITY_KEYS, build, fresh_output, provenance,
                    run_command, sha256, write_json)
from stacks import aggregate


def doctor():
    """Probe parent-launched tracing instead of inferring it from an executable."""
    result = {"schema_version": 1, "platform": platform.platform(),
              "python": sys.version, "cargo": shutil.which("cargo"),
              "gdb": {"available": False}, "perf": {"available": False}}
    if platform.system() == "Linux" and shutil.which("gdb") and Path("/bin/sleep").exists():
        probe = '''import gdb,threading,os,signal
gdb.execute("starti",to_string=True)
pid=gdb.selected_inferior().pid
t=threading.Timer(0.05,os.kill,(pid,signal.SIGINT));t.start()
try:
 gdb.execute("continue",to_string=True)
finally:
 t.cancel();t.join()
assert gdb.newest_frame() is not None
print("SWARMIE_TRACE_PROBE_OK")
gdb.execute("kill",to_string=True)
'''
        argv = ["gdb", "-nx", "-q", "--batch", "-ex", "set confirm off", "-ex",
                "set debuginfod enabled off", "-ex", "python exec(" + repr(probe) + ")",
                "--args", "/bin/sleep", "1"]
        try:
            completed = subprocess.run(argv, text=True, capture_output=True, timeout=15)
            result["gdb"] = {"available": completed.returncode == 0 and "SWARMIE_TRACE_PROBE_OK" in completed.stdout,
                             "argv": argv, "exit_code": completed.returncode,
                             "stdout": completed.stdout, "stderr": completed.stderr,
                             "version": subprocess.check_output(["gdb", "--version"], text=True).splitlines()[0]}
        except subprocess.TimeoutExpired:
            result["gdb"] = {"available": False, "error": "trace probe timed out"}
    if shutil.which("perf"):
        try:
            completed = subprocess.run(["perf", "stat", "-e", "task-clock", "--", "/bin/true"],
                                       text=True, capture_output=True, timeout=10)
            result["perf"] = {"available": completed.returncode == 0, "exit_code": completed.returncode,
                              "stderr": completed.stderr, "note": "capability probe only; retained backend is gdb"}
        except subprocess.TimeoutExpired:
            result["perf"] = {"available": False, "error": "perf probe timed out"}
    return result


def workload_command(binary, output, repetitions, observer, timeout):
    return [str(binary), "--output", str(output), "--repetitions", str(repetitions),
            "--observer", observer, "--diagnostic-timeout-secs", str(timeout)]


def valid_summary(path, expected_runs):
    data = json.loads((path / "summary.json").read_text())
    if (data.get("schema_version") != 1 or data.get("valid") is not True or
            len(data.get("runs", [])) != expected_runs or
            any(r.get("valid") is not True or r.get("error") is not None for r in data["runs"])):
        raise DiagnosticError("workload did not produce a valid complete schema-1 summary")
    return data


def benchmark_summary(summary):
    windows = [r["window"] for r in summary["runs"]]
    wall = [w["wall_ns"] / 1e9 for w in windows]
    cpu = [w["thread_cpu_ns"] / 1e9 for w in windows if w["thread_cpu_ns"] is not None]
    return {"runs": len(windows), "observer": summary["observer"],
            "wall_seconds": wall, "median_wall_seconds": statistics.median(wall),
            "thread_cpu_seconds": cpu, "median_thread_cpu_seconds": statistics.median(cpu) if cpu else None,
            "interpretation": "Unprofiled start-to-observed-terminal window, not exact completion or CI acceptance."}


def gdb_command(binary, argv):
    return ["gdb", "-nx", "-q", "--batch", "-ex", "set auto-load off",
            "-ex", "set debuginfod enabled off", "-x", str(Path(__file__).with_name("gdb_sampler.py")),
            "--args", str(binary), *argv[1:]]


def verify_capture(binary, digest, expected, output, label):
    actual = provenance(expected["profile"])
    write_json(output / f"provenance-after-{label}.json", actual)
    if sha256(binary) != digest:
        raise DiagnosticError("diagnostic binary changed during measurement")
    for key in SOURCE_IDENTITY_KEYS:
        if actual[key] != expected[key]:
            raise DiagnosticError("source inputs changed during measurement")


def execute(args, output):
    before = provenance(args.profile)
    write_json(output / "provenance-before-build.json", before)
    binary = build(output, args.profile, args.build_timeout)
    after = provenance(args.profile)
    write_json(output / "provenance.json", after)
    (output / "source-diff.patch").write_bytes(subprocess.check_output(["git", "diff", "HEAD", "--binary"], cwd=ROOT))
    if any(before[key] != after[key] for key in SOURCE_IDENTITY_KEYS):
        raise DiagnosticError("diagnostic/build inputs changed during build; retry after owners finish edits")
    binary_digest = sha256(binary)
    reference = output / "reference"
    argv = workload_command(binary, reference, args.repetitions, args.observer, args.diagnostic_timeout_secs)
    run_command(argv, output, "reference", args.process_timeout)
    summary = valid_summary(reference, args.repetitions)
    verify_capture(binary, binary_digest, after, output, "reference")
    benchmark = benchmark_summary(summary)
    write_json(output / "benchmark.json", benchmark)
    if args.command == "bench":
        return benchmark
    capability = doctor()
    write_json(output / "doctor.json", capability)
    if not capability["gdb"]["available"]:
        raise DiagnosticError("parent-launched GDB tracing unavailable; see doctor.json")
    config = {"output": str(output), "interval_ms": args.interval_ms,
              "max_samples": args.max_samples, "max_frames": args.max_frames}
    write_json(output / "sampler-config.json", config)
    env = dict(os.environ, SWARMIE_DIAGNOSTIC_GDB_CONFIG=str(output / "sampler-config.json"))
    argv = workload_command(binary, output / "profiled", args.repetitions, args.observer,
                            args.diagnostic_timeout_secs)
    run_command(gdb_command(binary, argv), output, "gdb", args.process_timeout, env=env)
    verify_capture(binary, binary_digest, after, output, "profile")
    profiled = valid_summary(output / "profiled", args.repetitions)
    sampler = json.loads((output / "sampler.json").read_text())
    if sampler["exit_code"] != 0 or len(sampler["windows"]) != args.repetitions:
        raise DiagnosticError("profile boundaries do not match requested repetitions")
    samples = [json.loads(line) for line in (output / "stacks.ndjson").read_text().splitlines()]
    if not samples:
        raise DiagnosticError("no stacks captured")
    report = aggregate(samples)
    report.update(binary_sha256=sha256(binary), backend="gdb", sampler=sampler,
                  profiled_windows=[r["window"] for r in profiled["runs"]],
                  unprofiled_reference=benchmark,
                  timing_limit="Debugger stops/symbol work perturb execution; profiled windows are not latency benchmarks.")
    write_json(output / "profile.json", report)
    lines = ["# Runtime stack observation", "", report["interpretation"], "", report["timing_limit"], "",
             f"Captured {len(samples)} samples; unresolved={report['unresolved_samples']}, "
             f"truncated={report['truncated_samples']}, unwind errors={report['unwind_error_samples']}.", "",
             f"Per-run sample cap reached: {sampler['max_samples_reached']}. Reaching the cap leaves later execution unsampled.", "",
             "## Inclusive stack presence", "", "| Function | Samples | Fraction |", "| --- | ---: | ---: |"]
    for row in report["inclusive"][:25]:
        lines.append(f"| `{row['function'].replace('|', '/').replace('`', '')}` | {row['samples']} | {row['sample_fraction']:.3f} |")
    lines += ["", "## Top frame", "", "| Function | Samples | Fraction |", "| --- | ---: | ---: |"]
    for row in report["top_frame"][:25]:
        lines.append(f"| `{row['function'].replace('|', '/').replace('`', '')}` | {row['samples']} | {row['sample_fraction']:.3f} |")
    (output / "profile.md").write_text("\n".join(lines) + "\n")
    return {"sample_count": len(samples), "output": str(output), "reference": benchmark}


def positive(value):
    number = int(value)
    if number <= 0:
        raise argparse.ArgumentTypeError("must be positive")
    return number


def parser():
    cli = argparse.ArgumentParser(description=__doc__)
    commands = cli.add_subparsers(dest="command", required=True)
    commands.add_parser("doctor", help="probe tools and actual parent-launched tracing")
    for name in ("bench", "profile"):
        command = commands.add_parser(name)
        command.add_argument("--output", required=True, type=Path)
        command.add_argument("--profile", default="dev", help="explicit Cargo profile; dev keeps opt0 defaults")
        command.add_argument("--repetitions", type=positive, default=1)
        command.add_argument("--observer", choices=("wait", "store"), default="wait")
        command.add_argument("--diagnostic-timeout-secs", type=positive, default=30)
        command.add_argument("--process-timeout", type=positive, default=180)
        command.add_argument("--build-timeout", type=positive, default=1800)
        if name == "profile":
            command.add_argument("--backend", choices=("gdb",), default="gdb")
            command.add_argument("--interval-ms", type=positive, default=25)
            command.add_argument("--max-samples", type=positive, default=128, help="maximum stack samples per repetition; later execution may be unsampled")
            command.add_argument("--max-frames", type=positive, default=128)
    compare = commands.add_parser("compare", help="compare retained unprofiled benchmarks; no workload execution")
    compare.add_argument("left", type=Path)
    compare.add_argument("right", type=Path)
    return cli


def main():
    args = parser().parse_args()
    output = None
    try:
        if args.command == "doctor":
            report = doctor()
            print(json.dumps(report, indent=2))
            return 0 if report["gdb"]["available"] and report["cargo"] else 1
        if args.command == "compare":
            rows = [json.loads((p / "benchmark.json").read_text()) for p in (args.left, args.right)]
            print(json.dumps({"left": rows[0], "right": rows[1],
                              "median_wall_delta_seconds": rows[1]["median_wall_seconds"] - rows[0]["median_wall_seconds"],
                              "interpretation": "Descriptive only; inspect provenance, observer, symbols, environment and semantic evidence before comparison."}, indent=2))
            return 0
        output = fresh_output(args.output)
        print(json.dumps(execute(args, output), indent=2))
        return 0
    except (DiagnosticError, OSError, ValueError, KeyError, subprocess.SubprocessError, KeyboardInterrupt) as error:
        if output is not None:
            write_json(output / "failure.json", {"schema_version": 1, "error": str(error),
                                                 "interrupted": isinstance(error, KeyboardInterrupt)})
        print(f"diagnostic failed: {error or 'interrupted'}", file=sys.stderr)
        return 130 if isinstance(error, KeyboardInterrupt) else 1


if __name__ == "__main__":
    sys.exit(main())
