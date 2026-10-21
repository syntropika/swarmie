"""Capture a real command outcome and immutable evidence, without declaring acceptance."""

import argparse
from datetime import datetime, timezone
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import time

from common import ROOT, digest, git, write_json


def source_identity(root):
    files = {}
    for name in git(root, "ls-files", "--cached", "--others", "--exclude-standard", "-z").split(b"\0"):
        if name:
            relative = name.decode()
            path = root / relative
            if path.is_symlink():
                files[relative] = {"symlink": os.readlink(path)}
            elif path.is_file():
                files[relative] = digest(path)
            else:
                files[relative] = None
    import hashlib
    import json
    return {"commit": git(root, "rev-parse", "HEAD").decode().strip(),
            "tree": git(root, "rev-parse", "HEAD^{tree}").decode().strip(),
            "dirty": bool(git(root, "status", "--porcelain")),
            "files": files,
            "files_sha256": hashlib.sha256(json.dumps(files, sort_keys=True).encode()).hexdigest()}


def run(root, output, command, criteria=(), artifacts=(), tools=(), timeout=None):
    output = Path(output).resolve()
    # A fresh directory prevents stale evidence from a previous pass masking failure.
    if output.exists():
        raise ValueError("evidence output must be a fresh directory")
    if output.is_relative_to(root.resolve()):
        raise ValueError("execution evidence belongs outside the source repository")
    output.mkdir(parents=True)
    before = source_identity(root)
    started = datetime.now(timezone.utc).isoformat()
    tool_versions = {}
    for tool in tools:
        executable = shutil.which(tool)
        if not executable:
            tool_versions[tool] = {"available": False}
            continue
        version = subprocess.run([executable, "--version"], capture_output=True, text=True, timeout=15)
        tool_versions[tool] = {"available": True, "executable_sha256": digest(Path(executable).resolve()),
                               "version": version.stdout.strip(), "version_exit_code": version.returncode}
    start = time.monotonic()
    error = None
    with (output / "command.log").open("wb") as log:
        try:
            process = subprocess.Popen(command, cwd=root, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
            try:
                code = process.wait(timeout=timeout)
            except (subprocess.TimeoutExpired, KeyboardInterrupt) as failure:
                os.killpg(process.pid, signal.SIGTERM)
                try:
                    process.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL)
                    process.wait()
                code = 124 if isinstance(failure, subprocess.TimeoutExpired) else 130
                error = "command interrupted or timed out; result is incomplete"
        except OSError as failure:
            code, error = 127, str(failure)
    after = source_identity(root)
    recorded = {}
    for pattern in artifacts:
        for path in sorted(root.glob(pattern)):
            if path.is_file():
                relative = path.relative_to(root).as_posix()
                # Save bytes as well as hashes so a later build cannot overwrite evidence.
                destination = output / "artifacts" / relative
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(path, destination)
                recorded[relative] = digest(destination)
    result = {"schema_version": 1, "started_utc": started, "elapsed_seconds": time.monotonic() - start,
              "argv": command, "exit_code": code, "error": error,
              "outcome": "passed" if code == 0 else "failed", "source_before": before,
              "source_after": after, "source_changed": before != after,
              "tools": tool_versions, "artifacts": recorded, "log_sha256": digest(output / "command.log"),
              "criteria_exercised": list(criteria), "acceptance": "not_determined",
              "limitations": ["Criteria labels identify intended command scope; a successful command does not automatically prove every criterion.",
                              "Success/rejection/recovery require explicit behavior assertions; counts or build success are insufficient.",
                              "No hosted, macOS, mobile device, performance or crash qualification is implied unless the executed command provides it."]}
    write_json(output / "result.json", result)
    return code


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True)
    parser.add_argument("--criterion", action="append", default=[])
    parser.add_argument("--artifact", action="append", default=[])
    parser.add_argument("--tool", action="append", default=[])
    parser.add_argument("--timeout", type=float)
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if not command:
        parser.error("an executable command is required")
    raise SystemExit(run(ROOT, args.output, command, args.criterion, args.artifact, args.tool, args.timeout))


if __name__ == "__main__":
    main()
