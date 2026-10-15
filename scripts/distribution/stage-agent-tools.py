#!/usr/bin/env python3
"""Explicitly stage pinned Claude SDK and Cursor Bridge dependencies for a runtime node."""
import argparse
import json
from pathlib import Path, PurePosixPath
import platform
import shutil
import subprocess
import tarfile
import tempfile
import urllib.request
from agent_tools import digest, stage_auth_worker

PROJECT = Path(__file__).resolve().parents[2]
CURSOR_VERSION = '1.0.37'
CURSOR_SHA256 = {
    'darwin-arm64': '2d0f69ba7086d32bce1173740a6b89ec5d5cbe377f3a4072a119a4e94bfcb71e',
    'darwin-x64': '731e2b521eb80316aba37e1d057f4daefebbcbd8ffb173dcb54409210be749ca',
    'linux-arm64': 'd12acaa7fc56c4007802c52b7ad1f7b68904faddd147c716647a3cd620dd841d',
    'linux-x64': '6076187d12b9e37dc68ad9736bb08424a78cde5acb3c2cd1fa706fa43a20ed5a',
    'win32-x64': 'c86cbd7a192b7538629efd36c96eaf3d11637d4618e12ebb3c3cd83d3fb80937',
}


def extract_bridge(archive, destination):
    with tarfile.open(archive, 'r:gz') as bundle:
        total = 0
        seen = set()
        for member in bundle:
            path = PurePosixPath(member.name)
            if path.is_absolute() or '..' in path.parts or not path.parts or path in seen or any('\\' in part or ':' in part for part in path.parts):
                raise SystemExit('Invalid Cursor Bridge archive path.')
            seen.add(path)
            target = destination.joinpath(*path.parts)
            if member.isdir():
                target.mkdir(parents=True, exist_ok=True)
                continue
            total += member.size
            if not member.isfile() or member.size < 0 or total > 512 * 1024 * 1024:
                raise SystemExit('Invalid Cursor Bridge archive member.')
            target.parent.mkdir(parents=True, exist_ok=True)
            with bundle.extractfile(member) as source, target.open('xb') as output:
                shutil.copyfileobj(source, output)
            target.chmod(0o755 if member.mode & 0o111 else 0o644)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True, help='Fresh support asset directory outside the source checkout')
    parser.add_argument('--tools', choices=['claude', 'cursor', 'auth', 'all'], default='all')
    parser.add_argument('--without-auth', action='store_true', help='Stage execution dependencies without optional browser authentication support')
    parser.add_argument('--node', default='node', help='Node.js 22 or newer executable available on the target runtime node')
    parser.add_argument('--npm', default='npm')
    parser.add_argument('--cursor-archive', type=Path, help='Use a cached official archive; its pinned checksum is still required')
    args = parser.parse_args()
    system = {'Linux': 'linux', 'Darwin': 'darwin', 'Windows': 'win32'}.get(platform.system())
    arch = {'x86_64': 'x64', 'amd64': 'x64', 'aarch64': 'arm64', 'arm64': 'arm64'}.get(platform.machine().lower())
    if system is None or arch is None:
        raise SystemExit('Agent tool staging supports Linux, macOS and Windows on x64 and arm64.')
    target = f'{system}-{arch}'
    output = args.output.expanduser().resolve()
    if output.is_relative_to(PROJECT) or output.exists():
        raise SystemExit('Support staging requires a fresh directory outside the project checkout.')
    with tempfile.TemporaryDirectory(prefix='swarmie-agent-tools-') as temporary:
        staged = Path(temporary) / 'agent-tools'
        staged.mkdir()
        versions = {}
        if not args.without_auth:
            versions['auth'] = stage_auth_worker(PROJECT / 'packages/agent-auth-worker', staged / 'auth', args.node, args.npm)
        if args.tools in ('claude', 'all'):
            version = subprocess.check_output([args.node, '--version'], text=True).strip()
            if not version.startswith('v') or int(version[1:].split('.')[0]) < 22:
                raise SystemExit('Claude SDK support requires Node.js 22 or newer on the runtime node.')
            worker = staged / 'claude'
            worker.mkdir()
            source = PROJECT / 'packages/claude-agent-worker'
            for name in ('package.json', 'package-lock.json'):
                shutil.copy2(source / name, worker / name)
            for module in source.glob('*.mjs'):
                shutil.copy2(module, worker / module.name)
            if not (worker / 'worker.mjs').is_file():
                raise SystemExit('Claude SDK worker entry point is missing.')
            # The worker uses the explicitly installed Claude CLI. Do not ship a
            # second optional platform CLI from the SDK dependency tree.
            subprocess.run([args.npm, 'ci', '--omit=dev', '--omit=optional', '--ignore-scripts', '--no-audit', '--no-fund'], cwd=worker, check=True)
            # npm bin aliases are symlinks. Dereference within the staging tree for
            # portable, regular-file-only archives and remote installation.
            for path in list(worker.rglob('*')):
                if path.is_symlink():
                    resolved = path.resolve()
                    if not resolved.is_relative_to(worker) or not resolved.is_file():
                        raise SystemExit('Unexpected Claude SDK dependency symlink.')
                    path.unlink()
                    shutil.copy2(resolved, path)
            versions['claude'] = json.loads((worker / 'package.json').read_text())['dependencies']['@anthropic-ai/claude-agent-sdk']
        if args.tools in ('cursor', 'all'):
            if target not in CURSOR_SHA256:
                raise SystemExit('No pinned official Cursor Bridge artifact is available for this platform.')
            filename = f'cursor-sdk-bridge-standalone-{target}.tar.gz'
            archive = args.cursor_archive or Path(temporary) / filename
            if args.cursor_archive is None:
                urllib.request.urlretrieve(f'https://github.com/cursor/sdk-bridge/releases/download/v{CURSOR_VERSION}/{filename}', archive)
            if digest(archive) != CURSOR_SHA256[target]:
                raise SystemExit('Cursor Bridge SHA-256 does not match the pinned official artifact.')
            cursor = staged / 'cursor'
            extract_bridge(archive, cursor)
            executable = cursor / 'bin' / ('cursor-sdk-bridge.exe' if system == 'win32' else 'cursor-sdk-bridge')
            if not executable.is_file():
                raise SystemExit('Official Cursor Bridge archive does not contain its executable.')
            versions['cursor'] = CURSOR_VERSION
        package_platform = {'darwin': 'macos', 'win32': 'windows'}.get(system, system) + '-' + {'x64': 'x86_64', 'arm64': 'aarch64'}.get(arch, arch)
        files = {path.relative_to(staged).as_posix(): digest(path) for path in sorted(staged.rglob('*')) if path.is_file()}
        (staged / 'agent-tools.json').write_text(json.dumps({'kind': 'swarmie-agent-tools', 'platform': package_platform, 'versions': versions, 'nodeMinimum': '22.13.0' if 'auth' in versions else 22 if 'claude' in versions else None, 'files': files}, indent=2) + '\n')
        output.parent.mkdir(parents=True, exist_ok=True)
        shutil.copytree(staged, output)
    print(json.dumps({'agentTools': str(output), 'platform': package_platform, 'versions': versions}, indent=2))


if __name__ == '__main__':
    main()
