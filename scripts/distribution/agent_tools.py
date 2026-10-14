"""Validate and copy explicitly staged runtime-owned SDK support assets."""
import hashlib
import json
from pathlib import Path
import shutil
import subprocess


def stage_auth_worker(source, destination, node='node', npm='npm'):
    """Stage only the locked SDK authentication worker, without logging in."""
    source, destination = Path(source), Path(destination)
    version = subprocess.check_output([node, '--version'], text=True).strip()
    try:
        major, minor = (int(part) for part in version.lstrip('v').split('.')[:2])
    except ValueError:
        raise SystemExit('Could not identify the authentication worker Node.js version.')
    if (major, minor) < (22, 13):
        raise SystemExit('Native SDK authentication requires Node.js 22.13 or newer.')
    destination.mkdir()
    for name in ('package.json', 'package-lock.json', 'worker.mjs'):
        shutil.copy2(source / name, destination / name)
    subprocess.run([npm, 'ci', '--omit=dev', '--omit=optional', '--ignore-scripts', '--no-audit', '--no-fund'], cwd=destination, check=True)
    for path in list(destination.rglob('*')):
        if path.is_symlink():
            resolved = path.resolve()
            if not resolved.is_relative_to(destination) or not resolved.is_file():
                raise SystemExit('Unexpected authentication SDK dependency symlink.')
            path.unlink()
            shutil.copy2(resolved, path)
    return json.loads((source / 'package.json').read_text())['dependencies']


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def copy_agent_tools(source, destination, host_platform=None):
    source = Path(source).expanduser().resolve()
    manifest = json.loads((source / 'agent-tools.json').read_text())
    if manifest.get('kind') != 'swarmie-agent-tools' or (host_platform and manifest.get('platform') != host_platform):
        raise SystemExit('Agent tool support assets do not match the package platform.')
    if destination.exists():
        raise SystemExit('Agent tool support destination already exists.')
    files = sorted(path for path in source.rglob('*') if path.is_file())
    for path in source.rglob('*'):
        if path.is_symlink() or (not path.is_file() and not path.is_dir()):
            raise SystemExit('Agent tool support assets must contain only regular files and directories.')
    expected = manifest.get('files')
    actual = {path.relative_to(source).as_posix(): digest(path) for path in files if path.name != 'agent-tools.json' or path.parent != source}
    if not isinstance(expected, dict) or expected != actual:
        raise SystemExit('Agent tool support SHA-256 inventory does not match staged files.')
    shutil.copytree(source, destination)
    return {'agent-tools/' + path.relative_to(source).as_posix(): digest(path) for path in files}
