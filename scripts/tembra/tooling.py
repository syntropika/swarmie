#!/usr/bin/env python3
"""Build and verify the exact public Tembra dependency outside the source tree."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
REVISION = "3779bc978f529ac9ade0871106715dda1d2a84a1"
SOURCE = "https://github.com/syntropika/tembra.git"
BUN_VERSION = "1.4.3"
RUST_VERSION = "1.99.0"
BINDGEN_VERSION = "0.2.129"
CACHE = Path(os.environ.get("SWARMIE_TEMBRA_CACHE", Path(os.environ.get("XDG_CACHE_HOME", Path.home() / ".cache")) / "swarmie" / "tembra")) / REVISION
CHECKOUT = CACHE / "source"
SDK = ROOT / "packages" / "tembra-sdk"


def run(argv, cwd=ROOT, capture=False):
    return subprocess.run([str(value) for value in argv], cwd=cwd, check=True, text=True,
                          stdout=subprocess.PIPE if capture else None).stdout


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, ensure_ascii=True) + "\n")


def inventory(directory):
    return {path.relative_to(directory).as_posix(): digest(path)
            for path in sorted(directory.rglob("*")) if path.is_file()}


def check_tools(browser=False):
    if run(["bun", "--version"], capture=True).strip() != BUN_VERSION:
        raise RuntimeError(f"Use the pinned Bun {BUN_VERSION} through the root toolchain")
    if browser:
        if not run(["rustc", f"+{RUST_VERSION}", "--version"], capture=True).startswith(f"rustc {RUST_VERSION} "):
            raise RuntimeError(f"Use the pinned Rust {RUST_VERSION} through the root toolchain")
        if run(["wasm-bindgen", "--version"], capture=True).strip() != f"wasm-bindgen {BINDGEN_VERSION}":
            raise RuntimeError(f"Install wasm-bindgen-cli {BINDGEN_VERSION}")


def checkout():
    CACHE.mkdir(parents=True, exist_ok=True)
    if not CHECKOUT.exists():
        run(["git", "clone", "--no-checkout", SOURCE, CHECKOUT])
        run(["git", "checkout", "--detach", REVISION], cwd=CHECKOUT)
    if run(["git", "rev-parse", "HEAD"], cwd=CHECKOUT, capture=True).strip() != REVISION:
        raise RuntimeError("The Tembra cache contains a different source revision")
    if run(["git", "status", "--porcelain"], cwd=CHECKOUT, capture=True).strip():
        raise RuntimeError("The Tembra source cache has modified or additional source files")
    return CHECKOUT


def bootstrap():
    check_tools()
    source = checkout()
    run(["bun", "install", "--frozen-lockfile"], cwd=source)
    run(["bun", "run", "--filter", "@tembra/sdk", "build"], cwd=source)
    files = inventory(source / "packages/sdk/dist/src")
    write_json(CACHE / "build.json", {"source": SOURCE, "revision": REVISION, "bun": BUN_VERSION,
                                    "files": files})
    print(source / "packages/sdk/dist/src/tooling/index.js")


def built_sdk():
    source = checkout()
    record = json.loads((CACHE / "build.json").read_text())
    if record["revision"] != REVISION or record["source"] != SOURCE or record["bun"] != BUN_VERSION:
        raise RuntimeError("Tembra bootstrap provenance differs")
    actual = inventory(source / "packages/sdk/dist/src")
    if actual != record["files"]:
        raise RuntimeError("Tembra generated SDK or tooling integrity differs; rebuild explicitly")
    return source / "packages/sdk/dist/src/tooling/index.js"


def project_sdk():
    built_sdk()
    source = CHECKOUT / "packages/sdk"
    upstream = SDK / "upstream"
    upstream.mkdir(parents=True, exist_ok=True)
    expected = inventory(source / "dist/src")
    for name in inventory(upstream):
        if name not in expected:
            raise RuntimeError(f"Unexpected SDK projection file: {name}")
    for name in expected:
        destination = upstream / name
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source / "dist/src" / name, destination)
    manifest = json.loads((source / "package.json").read_text())
    exports = {key: {kind: value.replace("./dist/src/", "./upstream/") for kind, value in entry.items()}
               for key, entry in manifest["exports"].items()}
    projection = {"name": manifest["name"], "version": manifest["version"],
               "private": True, "type": manifest["type"], "exports": exports,
               "dependencies": manifest["dependencies"], "engines": manifest["engines"]}
    write_json(SDK / "package.json", projection)
    write_json(SDK / "provenance.json", {"source": SOURCE, "revision": REVISION,
               "upstream_package_sha256": digest(source / "package.json"), "bun": BUN_VERSION,
               "package": projection, "files": expected})
    verify_sdk()


def verify_sdk():
    record = json.loads((SDK / "provenance.json").read_text())
    if record["revision"] != REVISION or record["source"] != SOURCE:
        raise RuntimeError("Unexpected SDK provenance")
    if inventory(SDK / "upstream") != record["files"]:
        raise RuntimeError("SDK projection differs from the recorded generated bytes")
    if json.loads((SDK / "package.json").read_text()) != record["package"]:
        raise RuntimeError("SDK exports or dependencies differ from the pinned projection")
    print("Verified the pinned SDK projection")


def prepare_sdk():
    """Prepare absent ignored dependency output; existing output must retain its provenance."""
    if not (SDK / "upstream").exists():
        bootstrap()
        project_sdk()
    else:
        verify_sdk()


def browser_wasm():
    check_tools(browser=True)
    source = checkout()
    run(["cargo", f"+{RUST_VERSION}", "build", "--locked", "-p", "tembra-browser-wasm", "--target", "wasm32-unknown-unknown", "--target-dir", source / "target"], cwd=source)
    output = ROOT / "target/browser-wasm"
    run(["wasm-bindgen", "--target", "web", "--out-dir", output, "--out-name", "tembra_browser_wasm",
         source / "target/wasm32-unknown-unknown/debug/tembra_browser_wasm.wasm"])
    write_json(output / "provenance.json", {"source": SOURCE, "revision": REVISION, "rust": RUST_VERSION,
               "wasm_bindgen": BINDGEN_VERSION, "files": {name: value for name, value in inventory(output).items() if name != "provenance.json"}})
    verify_browser()


def verify_browser():
    output = ROOT / "target/browser-wasm"
    record = json.loads((output / "provenance.json").read_text())
    if (record["source"], record["revision"], record["rust"], record["wasm_bindgen"]) != (SOURCE, REVISION, RUST_VERSION, BINDGEN_VERSION):
        raise RuntimeError("Browser WASM provenance differs from the pinned dependency")
    actual = {name: value for name, value in inventory(output).items() if name != "provenance.json"}
    if actual != record["files"]:
        raise RuntimeError("Browser WASM or glue integrity differs; rebuild explicitly")
    print("Verified the pinned Browser WASM and glue")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("operation", choices=["bootstrap", "project-sdk", "verify-sdk", "prepare-sdk", "browser-wasm", "verify-browser"])
    args = parser.parse_args()
    operations = {"bootstrap": bootstrap, "project-sdk": project_sdk, "verify-sdk": verify_sdk,
                  "prepare-sdk": prepare_sdk,
                  "browser-wasm": browser_wasm,
                  "verify-browser": verify_browser}
    operations[args.operation]()


if __name__ == "__main__":
    try:
        main()
    except (RuntimeError, subprocess.CalledProcessError, OSError, ValueError, KeyError) as error:
        print(f"Tembra tooling failed: {error}", file=sys.stderr)
        sys.exit(1)
