#!/usr/bin/env python3
"""Select native rebuilds using the last successfully uploaded signed binary's runtime."""

import argparse
import io
import json
import os
from pathlib import Path
import re
import urllib.error
import urllib.parse
import urllib.request
import zipfile

ARTIFACT_NAME = "ios-development-native-baseline"
WORKFLOW_PATH = ".github/workflows/mobile-ios.yml"


def runtime_version(value):
    runtime = value.get("runtimeVersion") if isinstance(value, dict) else None
    if not isinstance(runtime, str) or re.fullmatch(r"[a-f0-9]{40,64}", runtime) is None:
        raise ValueError("Expected a resolved native runtime fingerprint")
    return runtime


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, file, code, message, headers, url):
        return None


class GitHubArtifacts:
    def __init__(self, repository, token):
        if re.fullmatch(r"[\w.-]+/[\w.-]+", repository) is None:
            raise ValueError("Invalid repository identity")
        self.base = f"https://api.github.com/repos/{repository}"
        self.headers = {
            "Authorization": f"Bearer {token}",
            "Accept": "application/vnd.github+json",
            "User-Agent": "swarmie-mobile-build",
        }

    def json(self, path):
        request = urllib.request.Request(self.base + path, headers=self.headers)
        with urllib.request.urlopen(request, timeout=30) as response:
            return json.load(response)

    def candidates(self):
        query = urllib.parse.urlencode({"name": ARTIFACT_NAME, "per_page": 100})
        return self.json("/actions/artifacts?" + query)["artifacts"]

    def run(self, run_id):
        return self.json(f"/actions/runs/{int(run_id)}")

    def record(self, artifact):
        # GitHub redirects to a signed storage URL. Never forward its API token there.
        request = urllib.request.Request(
            self.base + f"/actions/artifacts/{int(artifact['id'])}/zip", headers=self.headers
        )
        try:
            urllib.request.build_opener(NoRedirect()).open(request, timeout=30)
        except urllib.error.HTTPError as error:
            if error.code != 302:
                raise
            location = error.headers["Location"]
        else:
            raise ValueError("Expected an artifact download redirect")
        target = urllib.parse.urlparse(location)
        if target.scheme != "https" or not target.hostname or target.username or target.password:
            raise ValueError("Invalid artifact download destination")
        with urllib.request.urlopen(urllib.request.Request(location), timeout=30) as response:
            content = response.read(1024 * 1024 + 1)
        if len(content) > 1024 * 1024:
            raise ValueError("Native baseline artifact is too large")
        with zipfile.ZipFile(io.BytesIO(content)) as archive:
            info = archive.getinfo("native-build.json")
            if info.file_size > 4096:
                raise ValueError("Native baseline record is too large")
            return json.loads(archive.read(info))


def baseline(source, branch):
    for artifact in source.candidates():
        if artifact.get("name") != ARTIFACT_NAME or artifact.get("expired", True):
            continue
        metadata = artifact.get("workflow_run") or {}
        if metadata.get("head_branch") != branch:
            continue
        run = source.run(metadata["id"])
        if (
            run.get("event") != "workflow_dispatch"
            or run.get("conclusion") != "success"
            or run.get("path") != WORKFLOW_PATH
            or run.get("head_branch") != branch
        ):
            continue
        record = source.record(artifact)
        if (
            record.get("platform") != "ios"
            or record.get("variant") != "development"
            or record.get("commit") != run.get("head_sha")
        ):
            raise ValueError("Native baseline does not match its signed build")
        runtime_version(record)
        return record
    return None


def plan(current, previous):
    runtime = runtime_version(current)
    if previous is None:
        return True, "No retained signed build baseline; a native build is required."
    if runtime != runtime_version(previous):
        return True, "Native inputs differ from the last signed build."
    return False, "Native runtime matches the last signed build; Expo export only."


def record_binary(ipa, expected, commit):
    with zipfile.ZipFile(ipa) as archive:
        runtime = archive.read("Payload/Swarmie.app/EXUpdates.bundle/fingerprint").decode().strip()
    record = {"platform": "ios", "variant": "development", "runtimeVersion": runtime, "commit": commit}
    if runtime_version(record) != expected:
        raise ValueError("The archived native runtime differs from the planned runtime")
    return record


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    selection = commands.add_parser("plan")
    selection.add_argument("--current", type=Path, required=True)
    capture = commands.add_parser("record")
    capture.add_argument("--ipa", type=Path, required=True)
    capture.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.command == "record":
        record = record_binary(args.ipa, os.environ["EXPECTED_RUNTIME_VERSION"], os.environ["GITHUB_SHA"])
        args.output.write_text(json.dumps(record, indent=2) + "\n")
        return
    current = json.loads(args.current.read_text())
    source = GitHubArtifacts(os.environ["GITHUB_REPOSITORY"], os.environ["GITHUB_TOKEN"])
    previous = baseline(source, os.environ["DEFAULT_BRANCH"])
    rebuild, reason = plan(current, previous)
    with Path(os.environ["GITHUB_OUTPUT"]).open("a") as stream:
        stream.write(f"rebuild={str(rebuild).lower()}\nruntime={runtime_version(current)}\n")
    with Path(os.environ["GITHUB_STEP_SUMMARY"]).open("a") as stream:
        stream.write(f"### Mobile build selection\n\n{reason}\n\nRuntime: `{runtime_version(current)}`\n")
    print(reason)


if __name__ == "__main__":
    try:
        main()
    except (ValueError, KeyError, OSError, zipfile.BadZipFile):
        raise SystemExit("Native build planning failed; no compatibility decision was made.") from None
