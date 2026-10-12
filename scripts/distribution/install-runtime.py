#!/usr/bin/env python3
"""Install, replace or remove runtime binaries without stopping processes or deleting data."""
import argparse
import fcntl
import hashlib
import html
import json
import os
from pathlib import Path
import platform
import shutil
import tempfile

MARKER = '.swarmie-runtime-install.json'
FILES = ('bin/swarmie', 'LICENSE', 'NOTICE', 'runtime-package.json')


def digest(path):
    hasher = hashlib.sha256()
    with path.open('rb') as file:
        for chunk in iter(lambda: file.read(1024 * 1024), b''):
            hasher.update(chunk)
    return hasher.hexdigest()


def require_plain_path(path):
    if path.is_symlink():
        raise SystemExit('Refusing symbolic-link installation path: ' + str(path))


def service_units():
    home = Path.home()
    config = Path(os.environ.get('XDG_CONFIG_HOME', home / '.config'))
    return (config / 'systemd/user/swarmie.service', home / 'Library/LaunchAgents/com.swarmie.daemon.plist')


def install(args):
    prefix = args.prefix.expanduser().absolute()
    bin_dir = args.bin_dir.expanduser().absolute()
    launcher = bin_dir / 'swarmie'
    target = prefix / 'bin/swarmie'
    for path in (prefix, prefix / 'bin', prefix / MARKER):
        require_plain_path(path)
    marker = prefix / MARKER
    saved = None
    if marker.exists():
        saved = json.loads(marker.read_text())
        if saved.get('kind') != 'swarmie-runtime-install' or set(saved.get('files', {})) != set(FILES):
            raise SystemExit('Invalid runtime installation marker.')
        if saved.get('launcher') != str(launcher):
            raise SystemExit('Use the original --bin-dir to update or uninstall this installation.')
        for relative, expected in saved['files'].items():
            path = prefix / relative
            require_plain_path(path)
            if not path.is_file() or digest(path) != expected:
                raise SystemExit('Refusing to replace or remove modified installation file: ' + relative)
    elif prefix.exists() and any(prefix.iterdir()):
        raise SystemExit('Refusing an unmanaged installation directory.')
    if launcher.exists() or launcher.is_symlink():
        if not launcher.is_symlink() or os.readlink(launcher) != str(target):
            raise SystemExit('Refusing to replace an unmanaged launcher: ' + str(launcher))
    if args.uninstall:
        if saved is None:
            raise SystemExit('No managed runtime installation exists at this prefix.')
        for unit in service_units():
            if unit.is_file():
                contents = html.unescape(unit.read_text())
                candidates = (str(prefix), str(prefix.resolve()))
                if any(value in contents or value.replace('\\', '\\\\').replace('\"', '\\\"').replace('%', '%%') in contents for value in candidates):
                    raise SystemExit('The service still references this installation. Explicitly stop and uninstall the service first.')
        if launcher.is_symlink():
            launcher.unlink()
        for relative in FILES:
            (prefix / relative).unlink()
        marker.unlink()
        # Remove only empty installation directories; never recursively delete user content.
        for directory in (prefix / 'bin', prefix):
            try:
                directory.rmdir()
            except OSError:
                pass
        print(json.dumps({'installed': False, 'dataPreserved': True, 'processesStopped': False}))
        return
    package = args.package.expanduser().resolve()
    for relative in FILES:
        path = package / relative
        if path.is_symlink() or not path.is_file():
            raise SystemExit('Incomplete runtime package: ' + relative)
    metadata = json.loads((package / 'runtime-package.json').read_text())
    system = {'Linux': 'linux', 'Darwin': 'macos'}.get(platform.system())
    arch = {'x86_64': 'x86_64', 'amd64': 'x86_64', 'aarch64': 'aarch64', 'arm64': 'aarch64'}.get(platform.machine())
    if metadata.get('kind') != 'swarmie-runtime' or metadata.get('platform') != f'{system}-{arch}':
        raise SystemExit('Runtime package does not match this host platform.')
    if digest(package / 'bin/swarmie') != metadata.get('executableSha256'):
        raise SystemExit('Runtime executable SHA-256 does not match the package metadata.')
    if 'Apache License' not in (package / 'LICENSE').read_text() or 'Copyright 2026 Syntropika' not in (package / 'NOTICE').read_text():
        raise SystemExit('The runtime package requires Apache-2.0 LICENSE and Syntropika NOTICE.')
    if not os.access(package / 'bin/swarmie', os.X_OK):
        raise SystemExit('Runtime executable is not executable.')
    prefix.mkdir(parents=True, exist_ok=True)
    (prefix / 'bin').mkdir(exist_ok=True)
    bin_dir.mkdir(parents=True, exist_ok=True)
    # Fully stage every file before atomically replacing the executable's stable path.
    # A running process retains its loaded executable; only subsequent starts use the update.
    with tempfile.TemporaryDirectory(prefix='.swarmie-runtime-stage-', dir=prefix.parent) as temporary:
        staging = Path(temporary)
        for relative in FILES:
            path = staging / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(package / relative, path)
        record = {'kind': 'swarmie-runtime-install', 'launcher': str(launcher),
                  'files': {relative: digest(staging / relative) for relative in FILES}}
        (staging / MARKER).write_text(json.dumps(record, indent=2) + '\n')
        for relative in FILES:
            os.replace(staging / relative, prefix / relative)
        os.replace(staging / MARKER, marker)
        # Stage on the launcher filesystem so replacement also works with separate mounts.
        descriptor, pending = tempfile.mkstemp(prefix='.swarmie-runtime-link-', dir=bin_dir)
        os.close(descriptor)
        pending = Path(pending)
        try:
            pending.unlink()
            pending.symlink_to(target)
            os.replace(pending, launcher)
        finally:
            if pending.is_symlink() or pending.exists():
                pending.unlink()
    print(json.dumps({'installed': True, 'updated': saved is not None, 'version': metadata['version'],
                      'prefix': str(prefix), 'launcher': str(launcher), 'dataPreserved': True,
                      'processesStopped': False, 'restartRequiredForRunningProcesses': saved is not None}))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--package', type=Path, default=Path(__file__).resolve().parent)
    parser.add_argument('--prefix', type=Path, default=Path.home() / '.local/share/swarmie/runtime')
    parser.add_argument('--bin-dir', type=Path, default=Path.home() / '.local/bin')
    parser.add_argument('--uninstall', action='store_true')
    args = parser.parse_args()
    prefix = args.prefix.expanduser().absolute()
    prefix.parent.mkdir(parents=True, exist_ok=True)
    lock = prefix.parent / ('.' + prefix.name + '-install.lock')
    descriptor = os.open(lock, os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
    with os.fdopen(descriptor, 'w') as file:
        fcntl.flock(file, fcntl.LOCK_EX)
        install(args)


if __name__ == '__main__':
    main()
