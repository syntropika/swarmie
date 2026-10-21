"""Inventory actual workspace feature and declared MSRV qualification inputs."""

import argparse
import tomllib

from architecture import load_metadata
from common import ROOT, write_json


def inventory(root, metadata):
    members = set(metadata["workspace_members"])
    packages = [package for package in metadata["packages"] if package["id"] in members]
    pinned = tomllib.loads((root / "mise.toml").read_text())["tools"]["rust"]
    declared = sorted({package["rust_version"] for package in packages if package["rust_version"]})
    missing = sorted(package["name"] for package in packages if not package["rust_version"])
    return {"schema_version": 1, "pinned_rust": pinned, "declared_rust_versions": declared,
            "missing_declared_rust_version": missing,
            "packages": {p["name"]: {"rust_version": p["rust_version"], "features": p["features"]} for p in packages},
            "profiles": ["workspace-default", "workspace-all-features"],
            "limitations": ["Default features are exercised by foundation-check; all-features is a separate integration lane.",
                            "All-features does not exercise every feature subset or every target triple.",
                            "A declared MSRV different from the pinned toolchain requires its own qualified lane; the inventory does not claim that lane passed."]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True)
    args = parser.parse_args()
    result = inventory(ROOT, load_metadata(ROOT))
    write_json(args.output, result)
    if result["missing_declared_rust_version"]:
        raise SystemExit("Every workspace package must declare its MSRV")
    # Only the actual pinned compiler is installed by this delivery. An MSRV
    # change must explicitly expand qualification instead of producing a false pass.
    normalized = {version if version.count(".") == 2 else version + ".0" for version in result["declared_rust_versions"]}
    if normalized != {result["pinned_rust"]}:
        raise SystemExit("Declared MSRV differs from the supported pinned qualification lane")


if __name__ == "__main__":
    main()
