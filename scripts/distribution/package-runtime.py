#!/usr/bin/env python3
"""Package a standalone headless runtime without the desktop host or frontend."""
import argparse
import hashlib
import json
from pathlib import Path
import platform
import shutil
import subprocess
import tarfile
import tempfile
import tomllib
import urllib.parse

PROJECT = Path(__file__).resolve().parents[2]


def digest(path):
    hasher = hashlib.sha256()
    with path.open('rb') as file:
        for chunk in iter(lambda: file.read(1024 * 1024), b''):
            hasher.update(chunk)
    return hasher.hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=Path(tempfile.gettempdir()) / 'swarmie-runtime-distribution')
    parser.add_argument('--skip-build', action='store_true')
    parser.add_argument('--binary', type=Path, help='Use an explicitly supplied matching native runtime executable')
    parser.add_argument('--release-base-url', help='HTTPS directory for the runtime-only updater manifest')
    parser.add_argument('--client-packages', type=Path, help='Also stage this archive and checksum in a native client runtime-package directory')
    parser.add_argument('--identity', help='macOS signing identity; omit to leave the runtime unsigned')
    args = parser.parse_args()
    system = {'Linux': 'linux', 'Darwin': 'macos', 'Windows': 'windows'}.get(platform.system())
    arch = {'x86_64': 'x86_64', 'amd64': 'x86_64', 'aarch64': 'aarch64', 'arm64': 'aarch64'}.get(platform.machine().lower())
    if system is None or arch is None:
        raise SystemExit('Runtime packaging supports Linux, macOS and Windows on x86_64 and aarch64.')
    if args.identity and system != 'macos':
        raise SystemExit('--identity requires a macOS host.')
    if args.release_base_url:
        url = urllib.parse.urlsplit(args.release_base_url)
        if url.scheme != 'https' or not url.netloc or url.username or url.password or url.query or url.fragment:
            raise SystemExit('Release base URL must be an HTTPS directory without credentials, query or fragment.')
    version = tomllib.loads((PROJECT / 'Cargo.toml').read_text())['workspace']['package']['version']
    executable = 'swarmie.exe' if system == 'windows' else 'swarmie'
    binary = (args.binary or PROJECT / 'target/release' / executable).expanduser().resolve()
    if not args.skip_build and args.binary is None:
        subprocess.run(['cargo', 'build', '--locked', '--release', '-p', 'app', '--bin', 'swarmie'], cwd=PROJECT, check=True)
    if not binary.is_file() or subprocess.check_output([binary, '--version'], text=True).strip() != 'swarmie ' + version:
        raise SystemExit('A native runtime executable matching the workspace version is required.')
    output = args.output.expanduser().resolve()
    output.mkdir(parents=True, exist_ok=True)
    name = f'swarmie-runtime-{version}-{system}-{arch}'
    bundle = output / name
    archive = output / (name + '.tar.gz')
    if bundle.exists() or archive.exists():
        raise SystemExit('Output already exists; use a fresh output directory.')
    (bundle / 'bin').mkdir(parents=True)
    shutil.copy2(binary, bundle / 'bin' / executable)
    if args.identity:
        subprocess.run(['codesign', '--force', '--options', 'runtime', '--timestamp', '--sign', args.identity, bundle / 'bin' / executable], check=True)
        subprocess.run(['codesign', '--verify', '--strict', bundle / 'bin' / executable], check=True)
    for filename in ('LICENSE', 'NOTICE'):
        shutil.copy2(PROJECT / filename, bundle / filename)
    shutil.copy2(PROJECT / 'scripts/distribution/install-runtime.py', bundle / 'install.py')
    metadata = {'kind': 'swarmie-runtime', 'version': version, 'platform': system + '-' + arch,
                'executableSha256': digest(bundle / 'bin' / executable)}
    (bundle / 'runtime-package.json').write_text(json.dumps(metadata, indent=2) + '\n')
    with tarfile.open(archive, 'w:gz') as result:
        result.add(bundle, arcname=name)
    checksum = digest(archive)
    (output / (archive.name + '.sha256')).write_text(checksum + '  ' + archive.name + '\n')
    if args.client_packages:
        client_packages = args.client_packages.expanduser().resolve()
        client_packages.mkdir(parents=True, exist_ok=True)
        destination = client_packages / (metadata['platform'] + '.tar.gz')
        if destination.exists() or destination.with_suffix('.gz.sha256').exists():
            raise SystemExit('Client runtime package already exists; use a fresh directory.')
        shutil.copy2(archive, destination)
        destination.with_suffix('.gz.sha256').write_text(checksum + '\n')
    if args.release_base_url:
        manifest = {'version': version, 'artifacts': {metadata['platform']: {
            'url': args.release_base_url.rstrip('/') + '/' + urllib.parse.quote(archive.name),
            'sha256': checksum, 'fileName': archive.name}}}
        (output / 'runtime-manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    print(json.dumps({'version': version, 'platform': metadata['platform'], 'archive': str(archive),
                      'sha256': checksum, 'desktopIncluded': False, 'signed': bool(args.identity)}, indent=2))


if __name__ == '__main__':
    main()
