#!/usr/bin/env python3
"""Own one isolated macOS daemon, controlled only over this SSH stdin channel."""
import hashlib
import json
import os
from pathlib import Path
import platform
import select
import signal
import shutil
import subprocess
import sys
import time


def main():
    binary, out, assets = (Path(value).resolve() for value in sys.argv[1:4])
    base = Path(sys.argv[-1]).resolve()
    if base not in out.parents or out.exists():
        raise SystemExit('A fresh owned remote research directory is required')
    out.mkdir(parents=True)
    shutil.copytree(assets, out / 'desktop-assets')
    assets = out / 'desktop-assets'
    (out / 'home').mkdir()
    (out / 'project').mkdir()
    (out / 'project/hello.txt').write_text('REMOTE_FILE_BASELINE\n')
    for command in [['git', 'init', '-b', 'main', str(out / 'project')],
                    ['git', '-C', str(out / 'project'), 'add', '.'],
                    ['git', '-C', str(out / 'project'), '-c', 'user.name=Fixture',
                     '-c', 'user.email=fixture@example.invalid', 'commit', '-m', 'Initialize controlled remote GUI project']]:
        subprocess.run(command, check=True, capture_output=True)
    initial_devices = json.loads(subprocess.check_output(['/usr/bin/xcrun', 'simctl', 'list', 'devices', '--json']))
    initial_states = {device['udid']: device['state'] for devices in initial_devices['devices'].values() for device in devices}
    (out / 'initial-devices.json').write_text(json.dumps(initial_devices, indent=2) + '\n')
    owned_devices = set()
    token_file = out / 'private-token'
    env = {key: os.environ[key] for key in ['PATH', 'USER', 'LOGNAME', 'LANG'] if key in os.environ}
    env.update(HOME=str(out / 'home'), XDG_CONFIG_HOME=str(out / 'config'),
               XDG_DATA_HOME=str(out / 'data'), XDG_CACHE_HOME=str(out / 'cache'), XDG_STATE_HOME=str(out / 'state'))
    daemon = None
    daemon_log = (out / 'daemon.log').open('a')
    port = None
    public_origin = None

    def stop():
        nonlocal daemon
        if daemon is not None and daemon.poll() is None:
            daemon.send_signal(signal.SIGINT)
            try:
                daemon.wait(timeout=15)
            except subprocess.TimeoutExpired:
                daemon.kill()
                daemon.wait()
                raise AssertionError('Remote daemon did not stop gracefully')
        return daemon.returncode if daemon else None

    def start(token):
        nonlocal daemon, port
        descriptor = os.open(token_file, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
        with os.fdopen(descriptor, 'w') as file:
            file.write(token + '\n')
        os.chmod(token_file, 0o600)
        daemon = subprocess.Popen([str(binary), '--state-dir', str(out / 'sessions'), 'serve',
            '--http', '127.0.0.1:' + str(port or 0), '--token-file', str(token_file), '--workbench',
            '--desktop-assets', str(assets), '--git-worktrees', str(out / 'worktrees'),
            '--resources-root', str(out / 'resources'),
            *(['--public-origin', public_origin] if public_origin else [])],
            env=env, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        end = time.monotonic() + 25
        while time.monotonic() < end:
            if daemon.poll() is not None:
                raise RuntimeError('Remote daemon exited during startup')
            if select.select([daemon.stdout], [], [], .2)[0]:
                line = daemon.stdout.readline()
                daemon_log.write(line)
                daemon_log.flush()
                if line.startswith('HTTP listening on '):
                    port = int(line.split('127.0.0.1:', 1)[1].split('/', 1)[0])
                    return {'port': port, 'pid': daemon.pid, 'project': str(out / 'project'),
                            'os': platform.system(), 'machine': platform.machine(),
                            'daemonSha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
                            'tokenFileMode': oct(token_file.stat().st_mode & 0o777)}
        raise TimeoutError('Remote daemon readiness')

    try:
        for line in sys.stdin:
            request = json.loads(line)
            action = request['action']
            if action in ['start', 'rotate']:
                if action == 'start':
                    public_origin = request.get('browserOrigin')
                if action == 'rotate':
                    stop()
                result = start(request['token'])
            elif action == 'verify':
                result = {'fileContent': (out / 'project/hello.txt').read_text(),
                          'terminalWritten': (out / 'project/terminal-written.txt').read_text()
                          if (out / 'project/terminal-written.txt').exists() else None,
                          'os': platform.system()}
            elif action == 'owned-device':
                device = request['deviceId']
                if initial_states.get(device) != 'Shutdown':
                    raise ValueError('Only an initially Shutdown simulator can be owned by this fixture')
                owned_devices.add(device)
                result = {'ownedDevice': device, 'initialState': initial_states[device]}
            elif action == 'devices':
                result = json.loads(subprocess.check_output(['/usr/bin/xcrun', 'simctl', 'list', 'devices', '--json']))
            elif action == 'stop':
                result = {'exitCode': stop(), 'stopped': True}
            else:
                raise ValueError('Unsupported owned control action')
            print(json.dumps(result), flush=True)
    finally:
        stop()
        cleanup = []
        for device in owned_devices:
            stopped = subprocess.run(['/usr/bin/xcrun', 'simctl', 'shutdown', device], capture_output=True, text=True)
            current = json.loads(subprocess.check_output(['/usr/bin/xcrun', 'simctl', 'list', 'devices', '--json']))
            actual = next((d['state'] for devices in current['devices'].values() for d in devices if d['udid'] == device), None)
            cleanup.append({'deviceId': device, 'restoredState': actual, 'exitCode': stopped.returncode})
        (out / 'cleanup.json').write_text(json.dumps({'ownedDevices': cleanup, 'daemonStopped': daemon is None or daemon.poll() is not None}, indent=2) + '\n')
        daemon_log.close()


if __name__ == '__main__':
    main()
