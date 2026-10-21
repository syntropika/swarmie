"""Output safety, ordinary Cargo discovery, and retained command provenance."""

import hashlib
import json
import os
from pathlib import Path
import platform
import signal
import subprocess
import time
import tomllib


ROOT = Path(__file__).resolve().parents[2]
PACKAGE = "swarmie-agent-loop-fixture"
EXAMPLE = "runtime-diagnostic"
SOURCE_IDENTITY_KEYS = ("head", "status", "tracked_diff_sha256", "repository_file_sha256")


class DiagnosticError(Exception):
    """An invalid, failed, or incomplete diagnostic operation."""


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")


def fresh_output(path, root=ROOT):
    path = Path(path).expanduser().resolve()
    if path == root or root in path.parents:
        raise DiagnosticError("raw output must be outside the checkout")
    path.parent.mkdir(parents=True, exist_ok=True)
    try:
        path.mkdir()
    except FileExistsError as error:
        raise DiagnosticError(f"output already exists: {path}") from error
    return path


def run_command(argv, output, label, timeout, env=None, cwd=ROOT):
    """Retain all output, kill the process group on timeout, and reject failure."""
    started = time.monotonic_ns()
    with (output / f"{label}.stdout.log").open("w") as stdout, (
        output / f"{label}.stderr.log"
    ).open("w") as stderr:
        process = subprocess.Popen(
            argv, cwd=cwd, env=env, stdout=stdout, stderr=stderr,
            start_new_session=True,
        )
        timed_out = False
        cleanup_error = None
        failure = None
        try:
            code = process.wait(timeout=timeout)
        except BaseException as exception:
            timed_out = isinstance(exception, subprocess.TimeoutExpired)
            failure = None if timed_out else exception
            if label == "gdb":
                cleanup_error = terminate_inferior(output, process.pid)
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            code = process.wait()
    record = {"argv": argv, "exit_code": code, "timed_out": timed_out,
              "interrupted": isinstance(failure, KeyboardInterrupt),
              "wait_error": str(failure) if failure else None,
              "wall_ns": time.monotonic_ns() - started, "cleanup_error": cleanup_error}
    write_json(output / f"{label}.command.json", record)
    if failure is not None:
        raise failure
    if timed_out or code:
        raise DiagnosticError(f"{label} failed (exit={code}, timeout={timed_out}); see retained logs")
    return record


def terminate_inferior(output, debugger_pid):
    """GDB may put its child in another process group; verify identity before killing."""
    record_path = output / "inferior.json"
    if not record_path.exists():
        return
    try:
        record = json.loads(record_path.read_text())
        stat = Path(f"/proc/{record['pid']}/stat")
        fields = stat.read_text().rsplit(") ", 1)[1].split()
        if int(fields[1]) == debugger_pid and fields[19] == record["start_ticks"]:
            os.kill(record["pid"], signal.SIGKILL)
    except ProcessLookupError:
        pass
    except FileNotFoundError:
        pass
    except (OSError, ValueError, KeyError) as error:
        return str(error)
    return None


def sha256(path):
    with Path(path).open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def capture(argv):
    return subprocess.check_output(argv, cwd=ROOT, text=True).strip()


def repository_hashes(root=ROOT):
    """Hash Git-listed inputs, including new modules outside the diagnostic fixture.

    Record hashes only, never untracked file contents or ignored local credentials/caches.
    Arbitrary extensions may be include_bytes!/build-script inputs, so do not filter suffixes.
    """
    names = subprocess.check_output([
        "git", "ls-files", "-z", "--cached", "--others", "--exclude-standard"], cwd=root)
    result = {}
    for name in sorted(set(os.fsdecode(names).split("\0")) - {""}):
        path = root / name
        if path.is_symlink() and root not in path.resolve().parents:
            raise DiagnosticError(f"cannot qualify an external symlink input: {name}")
        try:
            result[name] = sha256(path)
        except FileNotFoundError:
            result[name] = None
    return result


def provenance(profile):
    status = capture(["git", "status", "--porcelain=v1", "--untracked-files=all"])
    diff = subprocess.check_output(["git", "diff", "HEAD", "--binary"], cwd=ROOT)
    return {"schema_version": 1, "head": capture(["git", "rev-parse", "HEAD"]),
            "status": status, "tracked_diff_sha256": hashlib.sha256(diff).hexdigest(),
            "repository_file_sha256": repository_hashes(),
            "platform": platform.platform(), "logical_cpus": os.cpu_count(),
            "cpu_affinity": sorted(os.sched_getaffinity(0)) if hasattr(os, "sched_getaffinity") else None,
            "rustc": capture(toolchain_command("rustc") + ["--version"]),
            "cargo": capture(toolchain_command("cargo") + ["--version"]),
            "profile": profile, "environment": {k: v for k, v in os.environ.items()
                if k.startswith("CARGO_PROFILE_") or k in ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CARGO_TARGET_DIR", "CARGO_BUILD_TARGET", "CARGO_INCREMENTAL")}}


def cargo_command(profile):
    return toolchain_command("cargo") + ["build", "--locked", "-p", PACKAGE, "--example", EXAMPLE,
            "--profile", profile, "--message-format=json-render-diagnostics"]


def toolchain_command(tool):
    with (ROOT / "mise.toml").open("rb") as source:
        version = tomllib.load(source)["tools"]["rust"]
    return [tool, "+" + version]


def discover_binary(text):
    matches = []
    for line in text.splitlines():
        try:
            row = json.loads(line)
        except json.JSONDecodeError:
            continue
        if (row.get("reason") == "compiler-artifact" and
                row.get("target", {}).get("name") == EXAMPLE and
                "example" in row.get("target", {}).get("kind", []) and row.get("executable")):
            matches.append(row["executable"])
    if len(set(matches)) != 1:
        raise DiagnosticError("Cargo did not identify exactly one diagnostic executable")
    return Path(matches[0]).resolve()


def build(output, profile, timeout):
    run_command(cargo_command(profile), output, "build", timeout)
    messages = (output / "build.stdout.log").read_text()
    binary = discover_binary(messages)
    artifact_profile = None
    for line in messages.splitlines():
        try:
            row = json.loads(line)
        except json.JSONDecodeError:
            continue
        if row.get("executable") and Path(row["executable"]).resolve() == binary:
            artifact_profile = row.get("profile")
    write_json(output / "binary.json", {"path": str(binary), "sha256": sha256(binary),
                                        "cargo_artifact_profile": artifact_profile})
    return binary
