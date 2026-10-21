"""Bind named Rust scenario outcomes to a verified command receipt and source revision."""

import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess

from common import ROOT, digest, git, write_json


def command_receipt(receipt, log_hash):
    # Accept this tool's own immutable receipt and the coordinator's historical
    # structured step receipt. A standalone passing log is insufficient.
    if "steps" in receipt:
        matches = [step for step in receipt["steps"] if step.get("log_sha256") == log_hash]
        if len(matches) != 1:
            raise ValueError("log does not match exactly one executed command receipt")
        step = matches[0]
        return step["commit"], step["exit_code"], step["argv"], None
    if receipt.get("log_sha256") != log_hash or receipt.get("source_changed"):
        raise ValueError("command log identity or source stability is unverified")
    return receipt["source_before"]["commit"], receipt["exit_code"], receipt["argv"], receipt["source_before"]["files"]


def observations(log):
    found = {}
    target = None
    text = re.sub(r"\x1b\[[0-9;]*m", "", log)
    for line in text.splitlines():
        if "Running " in line and "(" in line:
            target = line.strip()
        match = re.fullmatch(r"test (\S+) \.\.\. (ok|FAILED|ignored)(?:,.*)?", line.strip())
        if match:
            outcome = {"ok": "passed", "FAILED": "failed", "ignored": "skipped"}[match[2]]
            found.setdefault(match[1], []).append({"outcome": outcome, "target": target})
    return found


def trace(root, manifest, log, receipt):
    log_hash = hashlib.sha256(log.encode()).hexdigest()
    revision, code, argv, files = command_receipt(receipt, log_hash)
    source_commit = git(root, "rev-parse", "--verify", f"{revision}^{{commit}}").decode().strip()
    found = observations(log)
    criteria = {}
    for criterion, branches in manifest["criteria"].items():
        criteria[criterion] = {}
        for branch, selector in branches.items():
            source, name = selector["source"], selector["test"]
            if Path(source).is_absolute() or ".." in Path(source).parts:
                raise ValueError("scenario source must be within the repository")
            try:
                source_bytes = git(root, "show", f"{source_commit}:{source}")
                source_hash = hashlib.sha256(source_bytes).hexdigest()
                verified = bool(re.search(rb"(?m)^(?:async )?fn " + re.escape(name.encode()) + rb"\(", source_bytes))
                if files is not None and files.get(source) != source_hash:
                    verified = False
            except subprocess.CalledProcessError:
                source_hash, verified = None, False
            matches = found.get(name, [])
            outcome = matches[0]["outcome"] if len(matches) == 1 else "ambiguous" if matches else "not_executed"
            if not verified:
                outcome = "source_unverified"
            criteria[criterion][branch] = {**selector, "outcome": outcome, "source_sha256": source_hash,
                                           "observations": matches}
    return {"schema_version": 1, "source_commit": source_commit, "command": argv, "command_exit_code": code,
            "log_sha256": log_hash, "criteria": criteria, "scope": manifest["scope"],
            "acceptance": "not_determined", "limitations": manifest["limitations"]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", default=str(ROOT / "tools/quality/runtime-cases.json"))
    parser.add_argument("--log", required=True)
    parser.add_argument("--receipt", required=True)
    parser.add_argument("--output", required=True)
    args = parser.parse_args()
    manifest = json.loads(Path(args.manifest).read_text())
    result = trace(ROOT, manifest, Path(args.log).read_bytes().decode(), json.loads(Path(args.receipt).read_text()))
    result["manifest_sha256"] = digest(Path(args.manifest))
    result["receipt_sha256"] = digest(Path(args.receipt))
    write_json(args.output, result)
    raise SystemExit(result["command_exit_code"] != 0 or any(case["outcome"] != "passed" for branches in result["criteria"].values() for case in branches.values()))


if __name__ == "__main__":
    main()
