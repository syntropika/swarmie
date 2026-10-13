#!/usr/bin/env python3
"""Install, replace or remove runtime binaries without stopping processes or deleting data."""
import argparse
import hashlib
import html
import json
import os
from pathlib import Path
import platform
import shutil
import stat
import sys
import subprocess
import tempfile

MARKER = '.swarmie-runtime-install.json'
WINDOWS = platform.system() == 'Windows'
EXECUTABLE = 'swarmie.exe' if WINDOWS else 'swarmie'
FILES = ('bin/' + EXECUTABLE, 'LICENSE', 'NOTICE', 'runtime-package.json')


def digest(path):
    hasher = hashlib.sha256()
    with path.open('rb') as file:
        for chunk in iter(lambda: file.read(1024 * 1024), b''):
            hasher.update(chunk)
    return hasher.hexdigest()


def require_plain_path(path):
    if path.is_symlink() or (path.exists() and getattr(path.lstat(), "st_file_attributes", 0) & getattr(stat, "FILE_ATTRIBUTE_REPARSE_POINT", 0x400)):
        raise SystemExit('Refusing symbolic-link installation path: ' + str(path))


def service_units():
    home = Path.home()
    config = Path(os.environ.get('XDG_CONFIG_HOME', home / '.config'))
    local = Path(os.environ.get('LOCALAPPDATA', home / 'AppData/Local'))
    return (config / 'systemd/user/swarmie.service', home / 'Library/LaunchAgents/com.swarmie.daemon.plist', local / 'swarmie/runtime-task.xml')


def windows_preflight(paths):
    if sys.platform != 'win32':
        return
    import ctypes
    from ctypes import wintypes
    api = ctypes.WinDLL('kernel32', use_last_error=True)
    api.CreateFileW.argtypes = [wintypes.LPCWSTR, wintypes.DWORD, wintypes.DWORD, ctypes.c_void_p, wintypes.DWORD, wintypes.DWORD, wintypes.HANDLE]
    api.CreateFileW.restype = wintypes.HANDLE
    api.CloseHandle.argtypes = [wintypes.HANDLE]
    handles = []
    try:
        for path in paths:
            if not path.exists():
                continue
            # A loaded image can still allow read/delete opens. Write access rejects its
            # image section before changing any managed file, including launcher copies.
            handle = api.CreateFileW(str(path), 0xC0010000, 0, None, 3, 0x80, None)
            if handle == wintypes.HANDLE(-1).value:
                raise SystemExit('An installation file is in use or protected. Explicitly stop execution before updating or removing it, then retry.')
            handles.append(handle)
    finally:
        for handle in handles:
            api.CloseHandle(handle)


def windows_service_reference(prefix, launcher):
    if sys.platform != 'win32':
        return False
    script = r"""
$ErrorActionPreference = 'Stop'
$prefix = [IO.Path]::GetFullPath($env:SWARMIE_INSTALL_PREFIX).TrimEnd('\') + '\'
$launcher = [IO.Path]::GetFullPath($env:SWARMIE_INSTALL_LAUNCHER)
foreach ($task in @(Get-ScheduledTask -ErrorAction Stop)) {
  foreach ($action in @($task.Actions)) {
    if ($action.Execute) {
      $command = [IO.Path]::GetFullPath([Environment]::ExpandEnvironmentVariables($action.Execute.Trim('"')))
      if ($command.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase) -or $command.Equals($launcher, [StringComparison]::OrdinalIgnoreCase)) {
        [Console]::Write('referenced'); exit 0
      }
    }
  }
}
[Console]::Write('clear')
"""
    environment = dict(os.environ, SWARMIE_INSTALL_PREFIX=str(prefix), SWARMIE_INSTALL_LAUNCHER=str(launcher))
    result = subprocess.run(['powershell.exe', '-NoLogo', '-NoProfile', '-NonInteractive', '-Command', script], env=environment, capture_output=True, timeout=15)
    if result.returncode or result.stdout not in (b'clear', b'referenced'):
        raise SystemExit('Scheduled task references could not be verified; preserve the installation and check Task Scheduler.')
    return result.stdout == b'referenced'


def install(args):
    prefix = args.prefix.expanduser().absolute()
    bin_dir = args.bin_dir.expanduser().absolute()
    launcher = bin_dir / (EXECUTABLE)
    target = prefix / 'bin' / EXECUTABLE
    for path in (prefix, prefix / 'bin', prefix / MARKER):
        require_plain_path(path)
    marker = prefix / MARKER
    saved = None
    if args.if_missing and marker.exists():
        raise SystemExit('A managed runtime installation now exists; check the connection again.')
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
        owned_launcher = (saved is not None and not launcher.is_symlink() and launcher.is_file() and digest(launcher) == saved.get('launcherSha256')) if WINDOWS else (launcher.is_symlink() and os.readlink(launcher) == str(target))
        if not owned_launcher:
            raise SystemExit('Refusing to replace an unmanaged launcher: ' + str(launcher))
    if WINDOWS and saved is not None:
        windows_preflight([prefix / relative for relative in FILES] + [launcher])
    if args.uninstall:
        if saved is None:
            raise SystemExit('No managed runtime installation exists at this prefix.')
        if WINDOWS and windows_service_reference(prefix, launcher):
            raise SystemExit('A scheduled task still references this installation. Explicitly stop and uninstall the service first.')
        for unit in service_units():
            if unit.is_file():
                contents = html.unescape(unit.read_text())
                candidates = (str(prefix), str(prefix.resolve()), str(launcher), str(launcher.resolve()))
                if any(value in contents or value.replace('\\', '\\\\').replace('\"', '\\\"').replace('%', '%%') in contents for value in candidates):
                    raise SystemExit('The service still references this installation. Explicitly stop and uninstall the service first.')
        if launcher.exists() or launcher.is_symlink():
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
    system = {'Linux': 'linux', 'Darwin': 'macos', 'Windows': 'windows'}.get(platform.system())
    arch = {'x86_64': 'x86_64', 'amd64': 'x86_64', 'aarch64': 'aarch64', 'arm64': 'aarch64'}.get(platform.machine().lower())
    if metadata.get('kind') != 'swarmie-runtime' or metadata.get('platform') != f'{system}-{arch}':
        raise SystemExit('Runtime package does not match this host platform.')
    if digest(package / 'bin' / EXECUTABLE) != metadata.get('executableSha256'):
        raise SystemExit('Runtime executable SHA-256 does not match the package metadata.')
    if 'Apache License' not in (package / 'LICENSE').read_text() or 'Copyright 2026 Syntropika' not in (package / 'NOTICE').read_text():
        raise SystemExit('The runtime package requires Apache-2.0 LICENSE and Syntropika NOTICE.')
    if not os.access(package / 'bin' / EXECUTABLE, os.X_OK):
        raise SystemExit('Runtime executable is not executable.')
    prefix.mkdir(parents=True, exist_ok=True)
    (prefix / 'bin').mkdir(exist_ok=True)
    bin_dir.mkdir(parents=True, exist_ok=True)
    # Fully stage every file before atomically replacing the executable's stable path.
    # Unix keeps loaded executables alive. Windows refuses replacement of an in-use file.
    with tempfile.TemporaryDirectory(prefix='.swarmie-runtime-stage-', dir=prefix.parent) as temporary:
        staging = Path(temporary)
        for relative in FILES:
            path = staging / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(package / relative, path)
        record = {'kind': 'swarmie-runtime-install', 'launcher': str(launcher),
                  'files': {relative: digest(staging / relative) for relative in FILES}}
        if WINDOWS:
            record['launcherSha256'] = digest(staging / 'bin' / EXECUTABLE)
        (staging / MARKER).write_text(json.dumps(record, indent=2) + '\n')
        for relative in FILES:
            try:
                os.replace(staging / relative, prefix / relative)
            except PermissionError:
                raise SystemExit('The runtime executable or an installation file is in use or protected. Explicitly stop execution before updating on Windows, then retry.') from None
        os.replace(staging / MARKER, marker)
        # Stage on the launcher filesystem so replacement also works with separate mounts.
        descriptor, pending = tempfile.mkstemp(prefix='.swarmie-runtime-link-', dir=bin_dir)
        os.close(descriptor)
        pending = Path(pending)
        try:
            if WINDOWS:
                shutil.copy2(target, pending)
            else:
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
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument('--uninstall', action='store_true')
    mode.add_argument('--if-missing', action='store_true', help='Refuse updates under the installation lock')
    args = parser.parse_args()
    prefix = args.prefix.expanduser().absolute()
    prefix.parent.mkdir(parents=True, exist_ok=True)
    lock = prefix.parent / ('.' + prefix.name + '-install.lock')
    require_plain_path(lock)
    descriptor = os.open(lock, os.O_RDWR | os.O_CREAT | getattr(os, 'O_NOFOLLOW', 0), 0o600)
    with os.fdopen(descriptor, 'r+b') as file:
        if os.name == 'nt':
            import msvcrt
            if file.seek(0, os.SEEK_END) == 0:
                file.write(b'\0')
                file.flush()
            file.seek(0)
            msvcrt.locking(file.fileno(), msvcrt.LK_LOCK, 1)
        else:
            import fcntl
            fcntl.flock(file, fcntl.LOCK_EX)
        install(args)


if __name__ == '__main__':
    main()
