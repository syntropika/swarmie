#!/usr/bin/env python3
"""Exercise a real package installer and HTTPS update CLI with synthetic public data."""
import argparse
import base64
import hashlib
import io
import json
from pathlib import Path
import platform
import subprocess
import tarfile
import urllib.parse


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--artifacts", type=Path, required=True)
    parser.add_argument("--https-origin", default="https://httpbin.org")
    args = parser.parse_args()
    root = args.artifacts.resolve()
    root.mkdir(parents=True, exist_ok=False)
    receipts = []

    def run(command, success=True):
        completed = subprocess.run([str(part) for part in command], capture_output=True, text=True, timeout=120)
        receipts.append({"command": [str(part) for part in command], "exitCode": completed.returncode, "stdout": completed.stdout, "stderr": completed.stderr})
        (root / "commands.json").write_text(json.dumps(receipts, indent=2) + "\n")
        if (completed.returncode == 0) != success:
            raise RuntimeError(completed.stderr or completed.stdout)
        return completed

    prefix = root / "application"
    install = ["python3", args.package / "install.py", "--package", args.package, "--prefix", prefix, "--bin-dir", root / "bin", "--desktop-dir", root / "desktop"]
    sentinel = root / "sessions-preserved.txt"
    sentinel.write_text("Owned fixture history remains outside the application.\n")
    run(install)
    for relative in ("bin/swarmie-desktop", "bin/swarmie-desktop_services", "resources/index.html"):
        assert hashlib.sha256((prefix / relative).read_bytes()).digest() == hashlib.sha256((args.package / relative).read_bytes()).digest()
    cli = root / "bin/swarmie"
    assert run([cli, "--version"]).stdout.strip() == "swarmie 0.1.0"
    run(install)

    buffer = io.BytesIO()
    content = b"Synthetic staged update fixture. No executable content.\n"
    with tarfile.open(fileobj=buffer, mode="w", format=tarfile.USTAR_FORMAT) as archive:
        member = tarfile.TarInfo("fixture.txt")
        member.size = len(content)
        archive.addfile(member, io.BytesIO(content))
    payload = buffer.getvalue()[:2048]
    with tarfile.open(fileobj=io.BytesIO(payload), mode="r:") as archive:
        assert archive.extractfile("fixture.txt").read() == content
    origin = args.https_origin.rstrip("/")
    assert urllib.parse.urlsplit(origin).scheme == "https"

    def url(data):
        return origin + "/base64/" + urllib.parse.quote(base64.b64encode(data).decode(), safe="")

    manifest = {"version": "0.2.0", "artifacts": {"linux-" + {"x86_64": "x86_64", "aarch64": "aarch64"}[platform.machine()]: {"url": url(payload), "sha256": hashlib.sha256(payload).hexdigest(), "fileName": "fixture-update.tar"}}}
    manifest_url = url(json.dumps(manifest).encode())
    checked = json.loads(run([cli, "--state-dir", root / "state", "update", "check", "--manifest", manifest_url]).stdout)
    assert checked["available"] and not checked["installed"]
    staged = json.loads(run([cli, "--state-dir", root / "state", "update", "stage", "--manifest", manifest_url, "--directory", root / "updates"]).stdout)
    assert staged["status"] == "staged" and not staged["installed"]
    assert Path(staged["path"]).read_bytes() == payload
    next(iter(manifest["artifacts"].values()))["sha256"] = "0" * 64
    rejected = run([cli, "--state-dir", root / "state", "update", "stage", "--manifest", url(json.dumps(manifest).encode()), "--directory", root / "bad-updates"], success=False)
    assert "SHA-256" in rejected.stderr or "checksum" in rejected.stderr.lower()
    assert not list((root / "bad-updates").rglob("*.partial"))
    assert not list((root / "bad-updates").rglob("fixture-update.tar"))
    run(install + ["--uninstall"])
    assert not prefix.exists() and not cli.is_symlink()
    assert sentinel.read_text() == "Owned fixture history remains outside the application.\n"
    (root / "result.json").write_text(json.dumps({"passed": True, "httpsOrigin": origin, "realPackageInstall": True, "sha256": hashlib.sha256(payload).hexdigest(), "checksumRejected": True, "payloadExtractedOrExecuted": False}, indent=2) + "\n")


if __name__ == "__main__":
    main()
