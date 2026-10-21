#!/usr/bin/env python3
"""Bounded shared-Host load campaigns, with retained failures and exact provenance."""

import argparse
from contextlib import contextmanager
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
import time

from common import (DiagnosticError, SOURCE_IDENTITY_KEYS, build, fresh_output, provenance,
                    run_command, sha256, write_json)
from diagnose import gdb_command, verify_capture
from stacks import aggregate


def bounded_integer(maximum):
    def parse(value):
        try:
            number = int(value)
        except ValueError as error:
            raise argparse.ArgumentTypeError("expected an integer") from error
        if not 1 <= number <= maximum:
            raise argparse.ArgumentTypeError(f"expected 1..{maximum}")
        return number
    return parse


def parser():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--output", type=Path, required=True)
    cli.add_argument("--profile", default="release")
    cli.add_argument("--resource-lock", type=Path, help="external advisory flock shared with other build/coverage/capture owners")
    cli.add_argument("--turns", type=bounded_integer(1024), default=128)
    cli.add_argument("--warmup-turns", type=bounded_integer(32), default=1)
    cli.add_argument("--concurrency", type=bounded_integer(128), nargs="+", default=[1, 2, 4, 8, 16])
    cli.add_argument("--arrival-interval-ms", type=bounded_integer(60000), nargs="+")
    cli.add_argument("--max-parallel", type=bounded_integer(128), default=16)
    cli.add_argument("--diagnostic-timeout-secs", type=bounded_integer(3600), default=120)
    cli.add_argument("--process-timeout", type=bounded_integer(30000), default=900)
    cli.add_argument("--campaign-timeout", type=bounded_integer(86400), default=1200,
                     help="total child-workload budget in seconds after build; incomplete levels remain explicit")
    cli.add_argument("--build-timeout", type=bounded_integer(30000), default=1800)
    cli.add_argument("--gdb-profile", action="store_true", help="separate sampled runs after each unprofiled reference; never used for capacity rates")
    cli.add_argument("--sample-interval-ms", type=bounded_integer(60000), default=25)
    cli.add_argument("--max-samples", type=bounded_integer(4096), default=128)
    return cli


@contextmanager
def resource_slot(path):
    """Nonblocking advisory exclusion; all host workload owners must use the same file."""
    if path is None:
        yield
        return
    if os.name != "posix":
        raise DiagnosticError("advisory resource locks require POSIX on this launcher")
    import fcntl
    resolved = path.expanduser().resolve()
    from common import ROOT
    if resolved == ROOT or ROOT in resolved.parents:
        raise DiagnosticError("resource lock must be outside the checkout")
    with resolved.open("a") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as error:
            raise DiagnosticError("shared resource slot is occupied; no build or capture started") from error
        try:
            yield
        finally:
            fcntl.flock(lock, fcntl.LOCK_UN)


def command(binary, destination, args, concurrency, interval):
    result = [str(binary), "--output", str(destination), "--turns", str(args.turns),
              "--concurrency", str(concurrency), "--warmup-turns", str(args.warmup_turns),
              "--max-parallel", str(args.max_parallel),
              "--diagnostic-timeout-secs", str(args.diagnostic_timeout_secs)]
    if interval is not None:
        result += ["--arrival-interval-ms", str(interval)]
    return result


def store_configuration(output):
    result = {}
    for name in ("tasks", "sessions", "bindings"):
        path = output / (name + ".sqlite")
        if path.exists():
            with sqlite3.connect(path.as_uri() + "?mode=ro", uri=True) as database:
                # journal_mode/application_id/user_version are persisted. synchronous is
                # connection-local; a new reader cannot prove the closed writer's setting.
                result[name] = {key: database.execute("PRAGMA " + key).fetchone()[0]
                                for key in ("journal_mode", "application_id", "user_version")}
            result[name]["writer_synchronous"] = None
            result[name]["writer_synchronous_scope"] = "not observable from a new read-only connection; inspect pinned provider source"
    return result


def execute(args, output):
    before = provenance(args.profile)
    write_json(output / "provenance-before-build.json", before)
    binary = build(output, args.profile, args.build_timeout)
    after = provenance(args.profile)
    write_json(output / "coordination.json", {"advisory_lock": str(args.resource_lock.resolve()) if args.resource_lock else None,
               "limit": "Advisory only; workload owners must coordinate the same file. A lock cannot detect unrelated workloads."})
    write_json(output / "provenance.json", after)
    for key in SOURCE_IDENTITY_KEYS:
        if before[key] != after[key]:
            raise DiagnosticError("source inputs changed during build")
    digest = sha256(binary)
    rows = []
    intervals = args.arrival_interval_ms or [None]
    planned_levels = len(args.concurrency) * len(intervals)
    deadline = time.monotonic() + args.campaign_timeout

    def campaign(complete):
        return {"schema_version": 1, "complete": complete,
                "valid": complete and all(row["valid"] and not (row.get("separate_profile") or {}).get("error") for row in rows),
                "planned_levels": planned_levels, "unstarted_levels": planned_levels-len(rows),
                "child_workload_budget_seconds": args.campaign_timeout,
                "levels": rows, "note": "Bounded descriptive campaign; no numeric SLO, confidence bound or extrapolation. Each level is a fresh process with one shared Host and store set. All failures/outliers retained."}
    for concurrency in args.concurrency:
        for interval in intervals:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                result = campaign(False)
                result["stop_reason"] = "campaign_budget_exhausted"
                write_json(output / "campaign.json", result)
                return result
            label = f"level-{len(rows)+1:03d}"
            destination = output / label
            error = None
            try:
                run_command(command(binary, destination, args, concurrency, interval),
                            output, label, min(args.process_timeout, remaining))
            except DiagnosticError as failure:
                error = str(failure)
            summary_path = destination / "summary.json"
            summary = json.loads(summary_path.read_text()) if summary_path.exists() else None
            row = {"label": label, "client_outstanding_cap": concurrency,
                   "arrival_interval_ms": interval, "command_error": error,
                   "valid": summary is not None and summary.get("valid") is True,
                   "statistics": summary.get("statistics") if summary else None,
                   "store_readback": store_configuration(destination)}
            rows.append(row)
            write_json(output / "campaign.json", campaign(False))
            verify_capture(binary, digest, after, output, label)
            if args.gdb_profile and row["valid"]:
                profile_output = output / (label + "-profile")
                profile_output.mkdir()
                config = profile_output / "gdb.json"
                write_json(config, {"output": str(profile_output), "interval_ms": args.sample_interval_ms,
                                    "max_samples": args.max_samples, "max_frames": 128})
                env = dict(os.environ, SWARMIE_DIAGNOSTIC_GDB_CONFIG=str(config))
                profile_error = None
                try:
                    argv = command(binary, profile_output / "workload", args, concurrency, interval)
                    remaining = deadline - time.monotonic()
                    if remaining <= 0:
                        raise DiagnosticError("campaign budget exhausted before separate profile")
                    run_command(gdb_command(binary, argv), profile_output, "gdb", min(args.process_timeout, remaining), env=env)
                    samples = [json.loads(line) for line in (profile_output / "stacks.ndjson").read_text().splitlines()]
                    write_json(profile_output / "attribution.json", aggregate(samples))
                except (DiagnosticError, OSError, ValueError) as failure:
                    profile_error = str(failure)
                row["separate_profile"] = {"path": str(profile_output), "error": profile_error,
                                           "timings_are_capacity_evidence": False}
                verify_capture(binary, digest, after, output, label + "-profile")
    result = campaign(True)
    write_json(output / "campaign.json", result)
    return result


def main():
    args = parser().parse_args()
    output = None
    try:
        output = fresh_output(args.output)
        with resource_slot(args.resource_lock):
            result = execute(args, output)
        print(json.dumps(result, indent=2))
        return 0 if result["valid"] else 1
    except (DiagnosticError, OSError, ValueError, KeyError, subprocess.SubprocessError, KeyboardInterrupt) as error:
        if output is not None:
            write_json(output / "failure.json", {"error": str(error), "interrupted": isinstance(error, KeyboardInterrupt)})
        print(f"load campaign failed: {error or 'interrupted'}", file=sys.stderr)
        return 130 if isinstance(error, KeyboardInterrupt) else 1


if __name__ == "__main__":
    sys.exit(main())
