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
