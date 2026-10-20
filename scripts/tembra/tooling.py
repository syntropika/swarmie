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
REVISION = "c394ec1f358a5054926f03409ac21f94d028617b"
SOURCE = "https://github.com/syntropika/tembra.git"
BUN_VERSION = "1.4.3"
RUST_VERSION = "1.99.0"
BINDGEN_VERSION = "0.2.129"
CACHE = Path(os.environ.get("SWARMIE_TEMBRA_CACHE", Path(os.environ.get("XDG_CACHE_HOME", Path.home() / ".cache")) / "swarmie" / "tembra")) / REVISION
CHECKOUT = CACHE / "source"
SDK = ROOT / "packages" / "tembra-sdk"
TARGET = ROOT / "target" / "tembra"


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


CONTRACTS = {"counter": "Counter", "remote-counter": "RemoteCounter",
             "dynamic-consumer": "DynamicConsumer",
             "remote-browser-access": "RemoteBrowserAccess",
             "plugin-configuration": "PluginConfiguration",
             "remote-plugin-administration": "RemotePluginAdministration"}

def generate_contracts(verify=False):
    built_sdk()
    verify_sdk()
    descriptors = TARGET / "contracts"
    run(["cargo", f"+{RUST_VERSION}", "run", "--locked", "--quiet", "-p", "swarmie-plugin-contracts", "--bin", "export-contract", "--", "--output-dir", descriptors])
    output = TARGET / "fixture-bindings-check" if verify else ROOT / "examples/plugins/generated"
    fixture_descriptor = TARGET / "fixture-contracts/dynamic-consumer-v2.json"
    fixture_descriptor.parent.mkdir(parents=True, exist_ok=True)
    fixture_descriptor.write_text(run(["cargo", f"+{RUST_VERSION}", "run", "--locked", "--quiet", "-p", "swarmie-plugin-contracts", "--bin", "export-contract", "--", "dynamic-consumer-v2"], capture=True))
    config = {"root": str(ROOT), "builds": {}, "codegen": [
        {"source": {"descriptor": str(descriptors / f"{name}.json")},
         "typescript": str(output / f"{name}.ts"), "name": token}
        for name, token in CONTRACTS.items()]}
    config["codegen"].append({"source": {"descriptor": str(fixture_descriptor)},
                              "typescript": str(output / "dynamic-consumer-v2.ts"),
                              "name": "DynamicConsumerV2"})
    path = TARGET / "contracts.config.ts"
    path.write_text("export default " + json.dumps(config, indent=2) + ";\n")
    run(["bun", ROOT / "scripts/tembra/build.ts", path])
    expected = inventory(output)
    if verify:
        if expected != inventory(ROOT / "examples/plugins/generated"):
            raise RuntimeError("Generated fixture bindings differ from the canonical Rust descriptors")
        print("Verified the canonical generated fixture bindings")
    else:
        write_json(TARGET / "fixture-bindings-provenance.json", {
            "tembra_revision": REVISION, "descriptor_source": "crates/plugin-contracts",
            "descriptors": inventory(descriptors), "files": expected})


def build_plugins():
    built_sdk()
    verify_sdk()
    result = run(["bun", ROOT / "scripts/tembra/build.ts",
                  "examples/plugins/tembra.config.ts"], capture=True)
    paths = {name: str(Path(path).relative_to(ROOT))
             for name, path in json.loads(result).items()}
    write_json(ROOT / "target/plugins/artifacts.json", paths)
    package_fixtures(paths)
    run(["bun", ROOT / "examples/plugins/browser/build.ts"])
    print(json.dumps(paths, indent=2))


def package_fixtures(paths):
    """Describe real Tembra outputs using the canonical Swarmie manifest DTO fields."""
    releases = [("dynamic-consumer-v1", "1.0.0"), ("dynamic-consumer-v2", "1.1.0"),
                ("dynamic-consumer-failing", "1.2.0"), ("dynamic-consumer-provider-removed", "1.3.0"),
                ("dynamic-consumer-evaluation-failure", "1.4.0"),
                ("dynamic-consumer-invalid-result", "1.5.0"),
                ("dynamic-consumer-invalid-argument", "1.6.0"),
                ("dynamic-consumer-incompatible", "1.7.0")]
    releases.append(("alternative-counter", "1.0.0"))
    for target, version in releases:
        plugin_id = "alternative-counter" if target == "alternative-counter" else "dynamic-consumer"
        destination = ROOT / "target/fixtures" / plugin_id / version
        destination.mkdir(parents=True, exist_ok=True)
        artifact = destination / "artifact.json"
        shutil.copyfile(ROOT / paths[target], artifact)
        envelope = json.loads(artifact.read_text())
        if envelope["format"] != "tembra-esm-v1" or hashlib.sha256(envelope["source"].encode()).hexdigest() != envelope["sha256"]:
            raise RuntimeError("The genuine tooling artifact failed source integrity verification")
        sha256 = digest(artifact)
        write_json(destination / "manifest.json", {
            "schema_version": 1, "id": plugin_id, "version": version,
            "compatibility": {"swarmie": "^0.1.0", "tembra_revision": REVISION},
            "dependencies": [],
            "facets": [{"id": plugin_id, "target": "server", "artifact_path": "artifact.json",
                        "sha256": sha256, "selected_export": "default"}],
            "files": [{"path": "artifact.json", "sha256": sha256}],
            "distribution": {"provider": "local", "description": "Contract-based Counter composition acceptance fixture"},
            "ui": []})


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
    parser.add_argument("operation", choices=["bootstrap", "project-sdk", "verify-sdk", "generate-contracts", "verify-contracts", "build-plugins", "browser-wasm", "verify-browser", "sdk-tooling"])
    args = parser.parse_args()
    operations = {"bootstrap": bootstrap, "project-sdk": project_sdk, "verify-sdk": verify_sdk,
                  "generate-contracts": generate_contracts, "verify-contracts": lambda: generate_contracts(verify=True), "build-plugins": build_plugins,
                  "browser-wasm": browser_wasm,
                  "verify-browser": verify_browser, "sdk-tooling": lambda: print(built_sdk())}
    operations[args.operation]()


if __name__ == "__main__":
    try:
        main()
    except (RuntimeError, subprocess.CalledProcessError, OSError, ValueError, KeyError) as error:
        print(f"Tembra tooling failed: {error}", file=sys.stderr)
        sys.exit(1)
