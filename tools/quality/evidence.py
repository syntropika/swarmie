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

from common import ROOT, committed_files, digest, git, write_json


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


def committed_snapshot(root, receipt):
    """Require a complete, stable clean snapshot attributable to its actual Git tree."""
    import hashlib
    import json
    before, after = receipt.get("source_before"), receipt.get("source_after")
    required = {"commit", "tree", "files_sha256", "dirty", "files"}
    if (not isinstance(before, dict) or not required <= before.keys() or before != after
            or receipt.get("source_changed") is not False):
        raise ValueError("command source snapshot is missing or changed during execution")
    if before.get("dirty") is not False:
        raise ValueError("dirty execution cannot qualify a committed source revision")
    files_hash = hashlib.sha256(json.dumps(before["files"], sort_keys=True).encode()).hexdigest()
    tree = git(root, "rev-parse", "--verify", f"{before['commit']}^{{tree}}").decode().strip()
    if (before.get("files_sha256") != files_hash or before.get("tree") != tree
            or before["files"] != committed_files(root, before["commit"])):
        raise ValueError("source snapshot digest or committed tree is inconsistent")
    return before


def produced_file(output, name):
    """Resolve an owned regular output without following links or accepting shared inodes."""
    import stat
    relative = Path(name)
    if relative.is_absolute() or not relative.parts or ".." in relative.parts or any(c in name for c in "*?["):
        raise ValueError("produced outputs require concrete relative file names")
    namespace = Path(output).resolve() / "produced"
    path = namespace / relative
    for part in (namespace, *[namespace.joinpath(*relative.parts[:index]) for index in range(1, len(relative.parts) + 1)]):
        if part.is_symlink():
            raise ValueError("produced outputs cannot contain symbolic links")
    status = path.stat()
    if not stat.S_ISREG(status.st_mode) or status.st_nlink != 1:
        raise ValueError("produced outputs must be new regular files without hard links")
    return path


def group_exists(process):
    process.poll()  # Reap the owned leader before checking its process group.
    try:
        os.killpg(process.pid, 0)
        return True
    except ProcessLookupError:
        return False


def stop_group(process):
    """Settle only the session/process group created for this child, with bounded escalation."""
    cleanup = {"requested": True, "process_group": process.pid, "term_sent": False, "kill_sent": False}
    for signum, field in ((signal.SIGTERM, "term_sent"), (signal.SIGKILL, "kill_sent")):
        try:
            os.killpg(process.pid, signum)
            cleanup[field] = True
        except ProcessLookupError:
            break
        deadline = time.monotonic() + 3
        while group_exists(process) and time.monotonic() < deadline:
            time.sleep(0.02)
        if not group_exists(process):
            break
    cleanup["group_settled"] = not group_exists(process)
    return cleanup


def execute(root, command, log, requested, timeout):
    process = None
    cleanup = {"requested": False, "group_settled": True}
    error = None
    if requested:
        return None, cleanup, "command not started after termination request", False
    try:
        process = subprocess.Popen(command, cwd=root, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
        deadline = time.monotonic() + timeout if timeout is not None else None
        while process.poll() is None:
            if requested or deadline is not None and time.monotonic() >= deadline:
                error = "command interrupted" if requested else "command timed out"
                break
            time.sleep(0.02)
        if error or group_exists(process):
            error = error or "owned descendants remained after the command exited"
            cleanup = stop_group(process)
        return process.poll(), cleanup, error, True
    except OSError as failure:
        if process is not None:
            cleanup = stop_group(process)
        return process.poll() if process else None, cleanup, str(failure), process is not None


def run(root, output, command, criteria=(), artifacts=(), tools=(), timeout=None, produces=()):
    output = Path(output).resolve()
    if output.exists():
        raise ValueError("evidence output must be a fresh directory")
    if output.is_relative_to(root.resolve()):
        raise ValueError("execution evidence belongs outside the source repository")
    for pattern in artifacts:
        if Path(pattern).is_absolute() or ".." in Path(pattern).parts:
            raise ValueError("captured artifacts must stay within the source repository")
    # This empty namespace belongs to this invocation. Existing user outputs are
    # captured only as inputs; they are never removed or promoted to producer proof.
    for name in produces:
        path = Path(name)
        if path.is_absolute() or not path.parts or ".." in path.parts or any(c in name for c in "*?["):
            raise ValueError("produced outputs require concrete relative file names")
    output.mkdir(parents=True)
    (output / "produced").mkdir()
    requested = []
    def terminate(signum, _frame):
        if not requested:
            requested.append(signum)
    previous = {signum: signal.signal(signum, terminate) for signum in (signal.SIGTERM, signal.SIGINT, signal.SIGHUP)}
    try:
        before = source_identity(root)
        event_context = {}
        event_file = os.environ.get("GITHUB_EVENT_PATH")
        if event_file:
            import json
            from plan import plan
            selected = plan(root, json.loads(Path(event_file).read_text()))
            event_context = {"base": selected["base"], "head": selected["head"],
                             "comparison_fallback": selected["fallback"],
                             "repository": os.environ.get("GITHUB_REPOSITORY"),
                             "run_id": os.environ.get("GITHUB_RUN_ID"),
                             "run_attempt": os.environ.get("GITHUB_RUN_ATTEMPT")}
        started = datetime.now(timezone.utc).isoformat()
        start = time.monotonic()
        tool_versions = {}
        for index, tool in enumerate(tools):
            executable = shutil.which(tool)
            if not executable or requested:
                tool_versions[tool] = {"available": False}
                continue
            with (output / f"tool-{index}.log").open("wb") as log:
                code, cleanup, error, _ = execute(root, [executable, "--version"], log, requested, 15)
            tool_versions[tool] = {"available": True, "executable_sha256": digest(Path(executable).resolve()),
                                   "version": (output / f"tool-{index}.log").read_text(errors="replace").strip(),
                                   "version_exit_code": code, "error": error, "cleanup": cleanup}
        argv = [argument.replace("{quality_output}", str(output / "produced")) for argument in command]
        with (output / "command.log").open("wb") as log:
            code, cleanup, error, command_started = execute(root, argv, log, requested, timeout)
        after = source_identity(root)
        recorded = {}
        for pattern in artifacts:
            for path in sorted(root.glob(pattern)):
                if path.is_file():
                    relative = path.relative_to(root).as_posix()
                    destination = output / "artifacts" / relative
                    destination.parent.mkdir(parents=True, exist_ok=True)
                    shutil.copyfile(path, destination)
                    recorded[relative] = digest(destination)
        produced, output_errors = {}, []
        for name in produces:
            try:
                produced[name] = digest(produced_file(output, name))
            except (OSError, ValueError) as failure:
                output_errors.append(f"{name}: {failure}")
        interrupted = requested[0] if requested else None
        runner_code = (128 + interrupted if interrupted else 124 if error == "command timed out" else
                       127 if not command_started else 128 - code if code is not None and code < 0 else code or 0)
        incomplete = bool(error or output_errors or before != after or not cleanup["group_settled"])
        if incomplete and not runner_code:
            runner_code = 1
        result = {"schema_version": 2, "started_utc": started, "elapsed_seconds": time.monotonic() - start,
                  "argv": argv, "requested_argv": command, "exit_code": code, "runner_exit_code": runner_code,
                  "command_started": command_started, "termination_signal": interrupted, "cleanup": cleanup,
                  "error": error, "output_errors": output_errors, "event_context": event_context,
                  "outcome": "interrupted" if interrupted else "passed" if runner_code == 0 else "failed",
                  "source_before": before, "source_after": after, "source_changed": before != after,
                  "tools": tool_versions, "artifacts": recorded, "artifact_role": "captured_inputs_not_producer_proof",
                  "produced_artifacts": produced, "output_provenance": "fresh_owned_namespace",
                  "log_sha256": digest(output / "command.log"), "criteria_exercised": list(criteria),
                  "acceptance": "not_determined",
                  "limitations": ["Fresh output creation and a successful command are not proof that the command instruments the intended code; the producer and its assertions require review.",
                                  "Captured artifacts are inputs/observations, never proof of newly produced coverage.",
                                  "Cleanup covers the owned process group, not descendants that deliberately escape into another session.",
                                  "SIGKILL, host loss and process failure before handler installation cannot guarantee a finalized receipt.",
                                  "Counts, coverage and criterion labels do not determine acceptance or qualify unlisted platforms and behaviors."]}
        write_json(output / "result.json", result)
        return runner_code
    finally:
        for signum, handler in previous.items():
            signal.signal(signum, handler)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True)
    parser.add_argument("--criterion", action="append", default=[])
    parser.add_argument("--artifact", action="append", default=[])
    parser.add_argument("--produce", action="append", default=[])
    parser.add_argument("--tool", action="append", default=[])
    parser.add_argument("--timeout", type=float)
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if not command:
        parser.error("an executable command is required")
    raise SystemExit(run(ROOT, args.output, command, args.criterion, args.artifact, args.tool, args.timeout, args.produce))


if __name__ == "__main__":
    main()
