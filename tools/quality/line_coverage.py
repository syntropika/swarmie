"""Summarize producer LCOV line data and added lines without hiding missing sources."""

import argparse
from pathlib import Path
import re
import subprocess

from common import ROOT, changed_paths, digest, git, write_json
from evidence import source_identity
import json

SOURCE_SUFFIXES = {".rs", ".ts", ".tsx", ".js", ".jsx", ".mjs", ".cjs", ".py", ".rb", ".sh"}


def source_path(root, value):
    path = Path(value)
    path = path if path.is_absolute() else root / path
    path = path.resolve()
    if not path.is_relative_to(root.resolve()):
        return None
    return path.relative_to(root.resolve()).as_posix()


def read_lcov(root, files):
    coverage = {}
    ignored = []
    for file in files:
        source = None
        for row in Path(file).read_text().splitlines():
            if row.startswith("SF:"):
                source = source_path(root, row[3:])
                if source is None:
                    ignored.append(row[3:])
                else:
                    coverage.setdefault(source, {})
            elif row.startswith("DA:"):
                if source is not None:
                    line, hits, *_ = row[3:].split(",")
                    line, hits = int(line), int(hits)
                    if line < 1 or hits < 0:
                        raise ValueError("invalid LCOV line or execution count")
                    coverage[source][line] = coverage[source].get(line, 0) + hits
            elif row == "end_of_record":
                source = None
    return coverage, ignored


def added_lines(root, base, head, paths):
    result = {}
    deleted = []
    for path in paths:
        if not (root / path).is_file():
            deleted.append(path)
            continue
        # Disable rename detection so added destination lines are not lost and removed
        # source lines never enter the changed-line denominator.
        patch = git(root, "diff", "--no-ext-diff", "--no-textconv", "--no-renames", "--unified=0", base, head, "--", path).decode()
        lines = set()
        for match in re.finditer(r"^@@ -\d+(?:,\d+)? \+(\d+)(?:,(\d+))? @@", patch, re.MULTILINE):
            start = int(match[1])
            count = int(match[2]) if match[2] is not None else 1
            lines.update(range(start, start + count))
        result[path] = lines
    return result, deleted


def counts(lines):
    total = len(lines)
    hit = sum(value > 0 for value in lines.values())
    return {"covered": hit, "executable": total, "percent": 100 * hit / total if total else None}


def report(root, lcov, base=None, evidence_paths=(), previous=None):
    head = git(root, "rev-parse", "HEAD").decode().strip()
    paths = [p.decode() for p in git(root, "ls-files", "--cached", "--others", "--exclude-standard", "-z").split(b"\0") if p]
    sources = {p for p in paths if Path(p).suffix in SOURCE_SUFFIXES and (root / p).is_file()}
    data, ignored = read_lcov(root, lcov)
    data = {p: rows for p, rows in data.items() if p in sources}
    totals = counts({(p, line): hits for p, rows in data.items() for line, hits in rows.items()})
    result = {"schema_version": 1, "commit": head,
              "working_tree_dirty": bool(git(root, "status", "--porcelain")),
              "reports": [{"name": Path(p).name, "sha256": digest(Path(p))} for p in lcov],
              "global_instrumented_lines": totals,
              "files": {p: counts(rows) for p, rows in data.items()},
              "uninstrumented_sources": sorted(sources - data.keys()),
              "outside_repository_lcov_sources": ignored,
              "source_scope": "all tracked supported source suffixes, including tests, fixtures and vendor sources; per-file inventory is authoritative",
              "policy": {"global": "trend_only_nonblocking", "changed_line_objective_percent": 90,
                         "stage": "baseline_capture_nonblocking", "baseline_established": False},
              "limitations": ["Line coverage is producer-reported execution, not branch, correctness or acceptance proof.",
                              "Files absent from LCOV are unmeasured, never treated as covered.",
                              "Added lines absent from an instrumented file's DA map are not in the executable denominator; this does not prove those lines cannot execute.",
                              "The 90 percent objective requires a qualified baseline before a blocking policy is activated.",
                              "Critical behavior requires separate success, rejection and recovery evidence."]}
    owners = sorted({str(Path(path).parent) for path in paths if Path(path).name in {"Cargo.toml", "package.json"}}, key=len, reverse=True)
    groups = {}
    for path in sources:
        owner = next((prefix for prefix in owners if Path(path).is_relative_to(prefix)), ".")
        group = groups.setdefault(owner, {"rows": {}, "unmeasured_sources": []})
        if path in data:
            group["rows"].update({(path, line): hits for line, hits in data[path].items()})
        else:
            group["unmeasured_sources"].append(path)
    result["packages"] = {owner: {**counts(group["rows"]), "unmeasured_sources": sorted(group["unmeasured_sources"])} for owner, group in groups.items()}
    result["trend"] = {"available": False, "reason": "no previous observation supplied"}
    if previous:
        prior = json.loads(Path(previous).read_text())
        before = prior["global_instrumented_lines"]["percent"]
        after = totals["percent"]
        result["trend"] = {"available": before is not None and after is not None,
                           "previous_commit": prior["commit"], "previous_report_sha256": digest(Path(previous)),
                           "percentage_point_change": after - before if before is not None and after is not None else None,
                           "instrumented_file_set_changed": set(prior["files"]) != set(data),
                           "limitation": "Observations can differ in instrumentation scope, code and tools; a trend is not a like-for-like regression proof."}
    result["producer_identity_verified"] = False
    if evidence_paths:
        expected = {entry["sha256"] for entry in result["reports"]}
        actual = set()
        current = source_identity(root)["files_sha256"]
        for evidence_path in evidence_paths:
            receipt = json.loads(Path(evidence_path).read_text())
            if receipt.get("exit_code") != 0 or receipt.get("source_changed"):
                raise ValueError("coverage producer receipt does not identify a successful immutable run")
            if receipt["source_before"]["files_sha256"] != current:
                raise ValueError("coverage producer source differs from the current source")
            actual.update(receipt.get("artifacts", {}).values())
        if not expected <= actual:
            raise ValueError("coverage producer receipts do not match every report's immutable bytes")
        result["producer_identity_verified"] = True
        result["producer_receipts_sha256"] = [digest(Path(path)) for path in evidence_paths]
    else:
        result["limitations"].append("Producer source identity is unverified without --evidence; this report cannot establish a qualified baseline.")
    try:
        if result["working_tree_dirty"]:
            raise ValueError("diff requires a committed source snapshot")
        if not base:
            raise ValueError("no comparison base")
        changed, deleted = added_lines(root, base, head, changed_paths(root, base, head))
        changed = {p: lines for p, lines in changed.items() if Path(p).suffix in SOURCE_SUFFIXES}
        measured = {(p, line): data[p][line] for p, lines in changed.items() if p in data
                    for line in lines if line in data[p]}
        result["diff"] = {"base": base, "available": True, **counts(measured),
                          "uncovered_lines": {p: sorted(line for line in lines if data[p].get(line) == 0)
                                              for p, lines in changed.items() if p in data},
                          "unmeasured_files": sorted(p for p, lines in changed.items() if lines and p not in data),
                          "unmapped_added_lines": {p: sorted(lines - data[p].keys()) for p, lines in changed.items() if p in data},
                          "deleted_paths_excluded": deleted}
    except (ValueError, UnicodeError, subprocess.CalledProcessError):
        result["diff"] = {"base": base, "available": False, "percent": None,
                          "reason": "comparison history unavailable or working tree is not committed"}
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--lcov", action="append", required=True)
    parser.add_argument("--base")
    parser.add_argument("--evidence", action="append", default=[])
    parser.add_argument("--previous")
    parser.add_argument("--output", required=True)
    args = parser.parse_args()
    result = report(ROOT, args.lcov, args.base, args.evidence, args.previous)
    write_json(args.output, result)
    print(result["global_instrumented_lines"])
    print(result["diff"])


if __name__ == "__main__":
    main()
