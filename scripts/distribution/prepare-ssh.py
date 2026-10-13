#!/usr/bin/env python3
"""Prepare a user-owned remote runtime over a bounded, verified SSH stream."""
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import platform
import shutil
import subprocess
import sys
import tarfile
import tempfile

LIMIT = 512 * 1024 * 1024
EXECUTABLE = 'swarmie.exe' if platform.system() == 'Windows' else 'swarmie'
FILES = {'bin/' + EXECUTABLE, 'LICENSE', 'NOTICE', 'runtime-package.json', 'install.py'}


def destination():
    system = {'Linux': 'linux', 'Darwin': 'macos', 'Windows': 'windows'}.get(platform.system())
    arch = {'x86_64': 'x86_64', 'amd64': 'x86_64', 'aarch64': 'aarch64', 'arm64': 'aarch64'}.get(platform.machine().lower())
    return f'{system}-{arch}' if system and arch else None


def executable():
    found = shutil.which(EXECUTABLE)
    managed = Path.home() / ('.local/share/swarmie/runtime/bin/swarmie.exe' if platform.system() == 'Windows' else '.local/bin/swarmie')
    return found or (str(managed) if managed.is_file() and os.access(managed, os.X_OK) else None)


def extract(archive, directory):
    # Accept only the runtime package's regular files. Never extract links or arbitrary paths.
    with tarfile.open(archive, 'r:gz') as bundle:
        found = set()
        root = None
        expanded = 0
        members = 0
        for member in bundle:
            members += 1
            if members > 16:
                raise ValueError('invalid-package')
            path = PurePosixPath(member.name)
            if path.is_absolute() or '..' in path.parts or not path.parts:
                raise ValueError('invalid-package')
            if root is None:
                root = path.parts[0]
            if path.parts[0] != root:
                raise ValueError('invalid-package')
            relative = '/'.join(path.parts[1:])
            if member.isdir() and relative in ('', 'bin'):
                continue
            if not member.isfile() or relative not in FILES or relative in found:
                raise ValueError('invalid-package')
            expanded += member.size
            if member.size < 0 or expanded > LIMIT or (relative != 'bin/' + EXECUTABLE and member.size > 1024 * 1024):
                raise ValueError('invalid-package')
            found.add(relative)
            target = directory / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            with bundle.extractfile(member) as source, target.open('xb') as output:
                shutil.copyfileobj(source, output)
            target.chmod(0o700 if relative in ('bin/' + EXECUTABLE, 'install.py') else 0o600)
        if found != FILES:
            raise ValueError('invalid-package')


def main():
    mode = sys.argv[1]
    if mode == 'probe':
        return {'platform': destination(), 'pythonReady': sys.version_info >= (3, 9),
                'installPath': str(Path.home() / '.local/share/swarmie/runtime')}
    if sys.version_info < (3, 9) or destination() is None:
        raise ValueError('unsupported-platform')
    if mode == 'install':
        checksum, target, version = sys.argv[2:5]
        if destination() != target:
            raise ValueError('platform-changed')
        if executable():
            raise ValueError('installation-changed')
        with tempfile.TemporaryDirectory(prefix='swarmie-ssh-') as temporary:
            directory = Path(temporary)
            archive = directory / 'runtime.tar.gz'
            digest = hashlib.sha256()
            size = 0
            with archive.open('xb') as output:
                while True:
                    chunk = sys.stdin.buffer.read(1024 * 1024)
                    if not chunk:
                        break
                    size += len(chunk)
                    if size > LIMIT:
                        raise ValueError('invalid-package')
                    digest.update(chunk)
                    output.write(chunk)
            if digest.hexdigest() != checksum:
                raise ValueError('checksum-mismatch')
            package = directory / 'package'
            package.mkdir()
            try:
                extract(archive, package)
                metadata = json.loads((package / 'runtime-package.json').read_text())
                if not isinstance(metadata, dict):
                    raise ValueError('invalid-package')
            except (tarfile.TarError, ValueError, OSError):
                raise ValueError('invalid-package') from None
            if metadata.get('kind') != 'swarmie-runtime' or metadata.get('platform') != target or metadata.get('version') != version:
                raise ValueError('invalid-package')
            installed = subprocess.run([sys.executable, str(package / 'install.py'), '--if-missing'],
                                       stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=60)
            if installed.returncode:
                raise ValueError('installation-refused')
        return {'installed': True}
    raise ValueError('invalid-action')


if __name__ == '__main__':
    try:
        result = main()
    except ValueError as error:
        result = {'error': str(error) if str(error) in {
            'unsupported-platform', 'platform-changed', 'installation-changed', 'invalid-package',
            'checksum-mismatch', 'installation-refused', 'invalid-action'} else 'preparation-failed'}
    except Exception:
        result = {'error': 'preparation-failed'}
    print(json.dumps(result))
    sys.exit(1 if 'error' in result else 0)
