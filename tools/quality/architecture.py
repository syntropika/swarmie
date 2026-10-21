"""Check declared production dependency ownership without compiling the workspace."""

import argparse
import ast
from pathlib import Path
import re
import tomllib

from common import ROOT, git, write_json


def load_metadata(root):
    # Cargo resolves features and target-specific optional edges. No build occurs.
    import subprocess
    import json
    workspace = tomllib.loads((root / "mise.toml").read_text())
    rust = workspace["tools"]["rust"]
    return json.loads(subprocess.check_output(
        ["cargo", f"+{rust}", "metadata", "--locked", "--all-features", "--format-version", "1"], cwd=root))


def dependency_graph(metadata):
    packages = {item["id"]: item for item in metadata["packages"]}
    graph = {}
    edges = []
    for node in metadata["resolve"]["nodes"]:
        graph[node["id"]] = set()
        for dependency in node["deps"]:
            # Retain normal and build edges, including target conditions. A dev-only
            # edge does not become production just because its destination is resolved.
            kinds = dependency["dep_kinds"]
            for kind in kinds:
                edges.append({"from": node["id"], "to": dependency["pkg"],
                              "kind": kind["kind"] or "normal", "target": kind["target"]})
            if any(kind["kind"] != "dev" for kind in kinds):
                graph[node["id"]].add(dependency["pkg"])
    return packages, graph, edges


def check(root, metadata=None):
    workspace = tomllib.loads((root / "Cargo.toml").read_text())
    metadata = load_metadata(root) if metadata is None else metadata
    packages, resolved, edges = dependency_graph(metadata)
    errors = []
    members = set(metadata["workspace_members"])
    manifests = {packages[identity]["name"]: identity for identity in members}
    # Keep package IDs throughout traversal so multiple versions cannot be collapsed.
    graph = resolved
    definitions = workspace["workspace"].get("dependencies", {})
    def closure(name, seen=None):
        seen = set() if seen is None else seen
        for dependency in graph.get(name, []):
            if dependency not in seen:
                seen.add(dependency)
                closure(dependency, seen)
        return seen

    for name, identity in manifests.items():
        dependency_ids = closure(identity)
        dependencies = {packages[item]["name"] for item in dependency_ids}
        for item in dependency_ids:
            package = packages[item]
            if package["source"] is None:
                destination = Path(package["manifest_path"]).resolve()
                if not destination.is_relative_to(root.resolve()):
                    errors.append(f"{name}: production dependency leaves public repository ({package['name']})")
                elif "tests" in destination.relative_to(root.resolve()).parts and not Path(packages[identity]["manifest_path"]).is_relative_to(root / "tests"):
                    errors.append(f"{name}: production dependency imports a test fixture ({package['name']})")
            if package["name"] in ("swarmie-cloud", "swarmie_cloud") or "swarmie-cloud" in (package["source"] or ""):
                errors.append(f"{name}: private hosted implementation dependency")
        if name.endswith("-contracts"):
            forbidden = {item for item in dependencies if item in manifests and not item.endswith("-contracts")}
            forbidden |= dependencies & {"rusqlite", "genai"}
            if forbidden:
                errors.append(f"{name}: portable Contracts depend on implementation: {', '.join(sorted(forbidden))}")
        if name == "swarmie-agent-loop-default":
            forbidden = dependencies & {"rusqlite", "genai", "swarmie-durable-tasks", "swarmie-plugin-manager"}
            forbidden |= {item for item in dependencies if item.startswith("swarmie-") and item in manifests and not item.endswith("-contracts")}
            if forbidden:
                errors.append(f"{name}: Agent loop imports concrete execution/storage/provider: {', '.join(sorted(forbidden))}")
        if name == "swarmie-durable-tasks" and "rusqlite" in dependencies:
            errors.append(f"{name}: scheduler depends on concrete SQLite storage")

    revisions = {spec.get("rev") for name, spec in definitions.items()
                 if name.startswith("tembra") and isinstance(spec, dict)}
    source = ast.parse((root / "scripts/tembra/tooling.py").read_text())
    revision = next((ast.literal_eval(node.value) for node in source.body
                     if isinstance(node, ast.Assign) and any(isinstance(t, ast.Name) and t.id == "REVISION" for t in node.targets)), None)
    if len(revisions) != 1 or revision not in revisions or not re.fullmatch(r"[0-9a-f]{40}", revision or ""):
        errors.append("Tembra Rust and SDK must share one immutable public revision")
    return {"schema_version": 1, "errors": errors, "metadata": metadata, "resolved_edges": edges,
            "limitations": ["Cargo metadata --locked --all-features resolves workspace optional features and all target conditions; normal/build closure is checked, dev-only edges are recorded but excluded. Dependencies outside workspace use Cargo-resolved features; this is not every possible downstream feature combination.",
                            "Static dependency checks do not prove runtime authority, lifecycle, absence of an alternate scheduler, or transaction semantics."]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True)
    args = parser.parse_args()
    result = check(ROOT)
    result["commit"] = git(ROOT, "rev-parse", "HEAD").decode().strip()
    write_json(args.output, result)
    for error in result["errors"]:
        print(error)
    raise SystemExit(bool(result["errors"]))


if __name__ == "__main__":
    main()
