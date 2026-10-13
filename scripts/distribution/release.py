#!/usr/bin/env python3
"""Validate a release tag and assemble verified desktop/runtime assets for one version."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import tomllib

ROOT = Path(__file__).resolve().parents[2]
PLATFORMS = ('linux-x86_64', 'linux-aarch64', 'macos-x86_64', 'macos-aarch64')


def validate_tag(tag):
    version = tomllib.loads((ROOT / 'Cargo.toml').read_text())['workspace']['package']['version']
    app = json.loads((ROOT / 'apps/desktop/app.json').read_text())['version']
    npm = json.loads((ROOT / 'apps/desktop/package.json').read_text())['version']
    if not re.fullmatch(r'\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?', version) or tag != 'v' + version or app != version or npm != version:
        raise ValueError('Release tag, workspace, desktop and npm versions must match.')
    return version


def digest(path):
    hasher = hashlib.sha256()
    with path.open('rb') as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b''):
            hasher.update(chunk)
    return hasher.hexdigest()


def assemble(artifacts, output, repository, tag, required):
    version = validate_tag(tag)
    if not required or len(set(required)) != len(required) or not set(required) <= set(PLATFORMS):
        raise ValueError('Select a nonempty set of unique supported platforms.')
    if not re.fullmatch(r'[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+', repository):
        raise ValueError('A repository owner/name is required.')
    base = f'https://github.com/{repository}/releases/download/{tag}/'
    manifests = {}
    files = {}
    for kind, manifest_name in [('desktop', 'manifest.json'), ('runtime', 'runtime-manifest.json')]:
        merged = {}
        for source in sorted(artifacts.rglob(manifest_name)):
            if source.is_symlink() or not source.is_file() or source.stat().st_size > 64 * 1024:
                raise ValueError('Invalid release manifest file.')
            manifest = json.loads(source.read_text())
            if not isinstance(manifest, dict) or set(manifest) != {'version', 'artifacts'} or manifest['version'] != version or not isinstance(manifest['artifacts'], dict):
                raise ValueError('Invalid or mixed-version release manifest.')
            for platform, artifact in manifest['artifacts'].items():
                if platform not in required or platform in merged:
                    raise ValueError('Unexpected or duplicate release platform.')
                extension = '.tar.gz' if kind == 'runtime' or platform.startswith('linux-') else '.zip'
                name = f'swarmie{"-runtime" if kind == "runtime" else ""}-{version}-{platform}{extension}'
                if not isinstance(artifact, dict) or set(artifact) != {'url', 'sha256', 'fileName'} or artifact['fileName'] != name or artifact['url'] != base + name:
                    raise ValueError('Artifact name or URL does not match the versioned release.')
                archive = source.parent / name
                sidecar = archive.with_name(name + '.sha256')
                if archive.is_symlink() or not archive.is_file() or archive.stat().st_size > 512 * 1024 * 1024:
                    raise ValueError('Missing, linked or oversized release archive.')
                if sidecar.is_symlink() or not sidecar.is_file() or sidecar.stat().st_size > 1024:
                    raise ValueError('Missing or invalid checksum sidecar.')
                checksum = digest(archive)
                if artifact['sha256'] != checksum or sidecar.read_text().split() != [checksum, name]:
                    raise ValueError('Release archive SHA-256 mismatch.')
                files[name] = archive
                files[name + '.sha256'] = sidecar
                merged[platform] = artifact
        if set(merged) != set(required):
            raise ValueError(f'{kind} release is missing required platforms.')
        manifests[manifest_name] = {'version': version, 'artifacts': merged}
    # Validate every input before producing any publishable output. Never overwrite an existing set.
    output.mkdir(parents=True, exist_ok=False)
    for name, source in sorted(files.items()):
        shutil.copy2(source, output / name)
    for name, manifest in manifests.items():
        (output / name).write_text(json.dumps(manifest, indent=2, sort_keys=True) + '\n')
    checksums = ''.join(f'{digest(path)}  {path.name}\n' for path in sorted(output.iterdir()))
    (output / 'SHA256SUMS').write_text(checksums)
    return {'version': version, 'tag': tag, 'platforms': list(required), 'assets': len(files) + 3}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='command', required=True)
    validate = commands.add_parser('validate-tag')
    validate.add_argument('--tag', required=True)
    build = commands.add_parser('assemble')
    build.add_argument('--tag', required=True)
    build.add_argument('--repository', required=True)
    build.add_argument('--artifacts', type=Path, required=True)
    build.add_argument('--output', type=Path, required=True)
    build.add_argument('--platform', action='append', choices=PLATFORMS)
    args = parser.parse_args()
    try:
        if args.command == 'validate-tag':
            result = {'version': validate_tag(args.tag), 'tag': args.tag}
        else:
            result = assemble(args.artifacts, args.output, args.repository, args.tag, tuple(args.platform or PLATFORMS))
    except (ValueError, OSError, KeyError, TypeError, IndexError) as error:
        parser.exit(1, f'{error}\n')
    print(json.dumps(result))


if __name__ == '__main__':
    main()
