"""Small, dependency-free utilities shared by repository quality commands."""

import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[2]


def git(root, *args):
    return subprocess.check_output(["git", "-C", str(root), *args], stderr=subprocess.PIPE)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def write_json(path, value):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")


def changed_paths(root, base, head="HEAD"):
    """Return both sides of renames; unavailable history raises rather than guesses."""
    git(root, "rev-parse", "--verify", f"{base}^{{commit}}")
    git(root, "rev-parse", "--verify", f"{head}^{{commit}}")
    raw = git(root, "diff", "--name-only", "--no-renames", "-z", base, head)
    return [name.decode("utf-8") for name in raw.split(b"\0") if name]


def committed_files(root, revision):
    """Read every immutable Git entry without checkout, export filters or a build."""
    entries = []
    for entry in git(root, "ls-tree", "-rz", "--full-tree", revision).split(b"\0"):
        if entry:
            header, name = entry.split(b"\t", 1)
            mode, kind, identity = header.split()
            entries.append((name.decode(), mode, kind, identity))
    blobs = [identity for _, _, kind, identity in entries if kind == b"blob"]
    data = subprocess.check_output(["git", "-C", str(root), "cat-file", "--batch"], input=b"\n".join(blobs) + b"\n") if blobs else b""
    result, offset = {}, 0
    for name, mode, kind, _ in entries:
        if kind != b"blob":
            result[name] = None
            continue
        end = data.index(b"\n", offset)
        size = int(data[offset:end].split()[2])
        content = data[end + 1:end + 1 + size]
        offset = end + size + 2
        result[name] = {"symlink": content.decode()} if mode == b"120000" else hashlib.sha256(content).hexdigest()
    return result
