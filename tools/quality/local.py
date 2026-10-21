"""Plan or execute bounded local feedback using Cargo's actual reverse dependency graph."""

import argparse
from pathlib import Path

from architecture import dependency_graph, load_metadata
from common import ROOT, changed_paths, git, write_json
from evidence import run
from plan import classify


def feedback(root, base):
    metadata = load_metadata(root)
    packages, _, edges = dependency_graph(metadata)
    members = set(metadata["workspace_members"])
    try:
        paths = set(changed_paths(root, base))
        paths.update(p.decode() for p in git(root, "diff", "--name-only", "--no-renames", "-z", "HEAD").split(b"\0") if p)
        paths.update(p.decode() for p in git(root, "ls-files", "--others", "--exclude-standard", "-z").split(b"\0") if p)
        families, reasons = classify(paths)
    except Exception as error:
        # A failed resolver is not caught here: architecture must fail honestly.
        # Only unavailable change history falls back to the full member set.
        import subprocess
        if not isinstance(error, (subprocess.CalledProcessError, UnicodeError, ValueError)):
            raise
        paths, families, reasons = set(), {"foundation": True, "frontend": True}, ["comparison unavailable"]
    selected = set()
    if reasons:
        selected = members.copy()
    else:
        for identity in members:
            directory = Path(packages[identity]["manifest_path"]).parent.relative_to(root)
            if any(Path(path).is_relative_to(directory) for path in paths):
                selected.add(identity)
        # Development edges matter for affected tests, including conformance fixtures.
        while True:
            expanded = selected | {edge["from"] for edge in edges if edge["to"] in selected and edge["from"] in members}
            if expanded == selected:
                break
            selected = expanded
    names = sorted(packages[identity]["name"] for identity in selected)
    import tomllib
    rust = tomllib.loads((root / "mise.toml").read_text())["tools"]["rust"]
    commands = [["python3", "tools/quality/architecture.py", "--output", "target/quality/local-architecture.json"],
                ["python3", "-m", "unittest", "discover", "-s", "tools/quality", "-p", "test_*.py"]]
    if families["foundation"]:
        commands.append(["cargo", f"+{rust}", "fmt", "--all", "--check"])
        if names:
            selectors = [value for name in names for value in ("-p", name)]
            commands.extend([["cargo", f"+{rust}", "clippy", "--locked", *selectors, "--all-targets", "--", "-D", "warnings"],
                             ["cargo", f"+{rust}", "test", "--locked", *selectors]])
    if families["frontend"]:
        commands.extend([["bun", "run", "format:check"], ["bun", "run", "lint"], ["bun", "run", "test"]])
    return {"schema_version": 1, "paths": sorted(paths), "families": families, "full_fallback_reasons": reasons,
            "affected_rust_packages": names, "commands": commands,
            "diff_coverage": "Run the coverage producer for affected code, then quality coverage with source evidence; omitted coverage remains unmeasured.",
            "limitations": ["Local feedback does not replace the integration gate or qualify browser/native devices.",
                            "Frontend feedback retains the existing root policy and test scope; package-granular frontend selection is not qualified."]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", default="HEAD")
    parser.add_argument("--output", required=True)
    parser.add_argument("--execute", action="store_true")
    args = parser.parse_args()
    output = Path(args.output).resolve()
    if output.is_relative_to(ROOT):
        parser.error("local output must be outside the source repository")
    result = feedback(ROOT, args.base)
    write_json(output / "plan.json", result)
    print("Affected Rust packages:", ", ".join(result["affected_rust_packages"]) or "none")
    for index, command in enumerate(result["commands"]):
        print(" ".join(command))
        if args.execute:
            code = run(ROOT, output / f"command-{index:02}", command, ["local.feedback"], tools=["cargo", "rustc", "bun", "python3"])
            if code:
                raise SystemExit(code)


if __name__ == "__main__":
    main()
