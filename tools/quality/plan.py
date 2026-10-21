"""Conservative change selection for complete verification families."""

import argparse
import json
import os
from pathlib import PurePosixPath
import subprocess

from common import ROOT, changed_paths, git, write_json

FAMILIES = ("foundation", "frontend")


def classify(paths):
    selected = set()
    reasons = []
    for path in paths:
        parts = PurePosixPath(path).parts
        filename = PurePosixPath(path).name
        foundational = (filename in {"Cargo.toml", "Cargo.lock", "package.json", "bun.lock", "mise.toml", "justfile", "bunfig.toml"}
                        or filename.startswith(("tsconfig", "oxlint", "oxfmt"))
                        or path.startswith((".github/", "packages/config/", "tools/quality/")))
        if foundational:
            selected.update(FAMILIES)
            reasons.append(f"full verification: {path}")
            continue
        if path in ("README.md", "LICENSE", "NOTICE") or path.startswith("assets/brandkit/"):
            # Brand assets can affect frontend bundling.
            if path.startswith("assets/"):
                selected.add("frontend")
            continue
        if path.startswith(("crates/", "plugins/")) and not path.endswith("Cargo.toml"):
            selected.add("foundation")
        elif path.startswith(("apps/web/", "apps/mobile/", "packages/ui/")):
            selected.add("frontend")
        elif path.startswith(("packages/tembra-sdk/", "scripts/tembra/", "tests/plugins/")):
            selected.update(FAMILIES)
        else:
            # Includes tool/config/dependency/workflow changes, distribution tooling,
            # unknown new paths and non-UTF8 paths rejected by changed_paths.
            reasons.append(f"full verification: {path}")
            selected.update(FAMILIES)
        if ".." in parts or PurePosixPath(path).is_absolute():
            raise ValueError("unsafe repository path")
    return {name: name in selected for name in FAMILIES}, reasons


def plan(root, event, requested_base=None):
    head = git(root, "rev-parse", "HEAD").decode().strip()
    base = requested_base
    try:
        if event.get("pull_request"):
            base = event["pull_request"]["base"]["sha"]
            base = git(root, "merge-base", base, head).decode().strip()
        elif event.get("before"):
            base = event["before"]
        if not base or set(base) == {"0"}:
            raise ValueError("no usable comparison base")
        paths = changed_paths(root, base, head)
        families, reasons = classify(paths)
        return dict(schema_version=1, head=head, base=base, paths=paths,
                    families=families, reasons=reasons, fallback=False)
    except (KeyError, ValueError, UnicodeError, subprocess.CalledProcessError) as error:
        return dict(schema_version=1, head=head, base=base, paths=[],
                    families=dict.fromkeys(FAMILIES, True), fallback=True,
                    reasons=[f"full verification: comparison unavailable ({type(error).__name__})"])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base")
    parser.add_argument("--output", required=True)
    parser.add_argument("--github-output")
    args = parser.parse_args()
    event_path = os.environ.get("GITHUB_EVENT_PATH")
    event = json.loads(open(event_path).read()) if event_path else {}
    result = plan(ROOT, event, args.base)
    write_json(args.output, result)
    if args.github_output:
        with open(args.github_output, "a") as output:
            for key, value in result["families"].items():
                output.write(f"{key}={str(value).lower()}\n")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
