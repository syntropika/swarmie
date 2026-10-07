#!/usr/bin/env python3
"""Own one isolated macOS daemon, controlled only over this SSH stdin channel."""
import hashlib
import json
import os
from pathlib import Path
import platform
import select
import signal
import subprocess
import sys
import time


def main():
    binary, out = Path(sys.argv[1]).resolve(), Path(sys.argv[2]).resolve()
    base = Path(sys.argv[-1]).resolve()
    if base not in out.parents or out.exists():
        raise SystemExit('A fresh owned remote research directory is required')
    out.mkdir(parents=True)
    (out / 'home').mkdir()
    (out / 'project').mkdir()
    (out / 'project/hello.txt').write_text('REMOTE_FILE_BASELINE\n')
    token_file = out / 'private-token'
    env = {key: os.environ[key] for key in ['PATH', 'USER', 'LOGNAME', 'LANG'] if key in os.environ}
    env.update(HOME=str(out / 'home'), XDG_CONFIG_HOME=str(out / 'config'),
               XDG_DATA_HOME=str(out / 'data'), XDG_CACHE_HOME=str(out / 'cache'), XDG_STATE_HOME=str(out / 'state'))
    daemon = None
    daemon_log = (out / 'daemon.log').open('a')
    port = None

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
            '--http', '127.0.0.1:' + str(port or 0), '--token-file', str(token_file), '--workbench'],
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
                if action == 'rotate':
                    stop()
                result = start(request['token'])
            elif action == 'verify':
                result = {'apiWritten': (out / 'project/api-written.txt').read_text(),
                          'terminalWritten': (out / 'project/terminal-written.txt').read_text(), 'os': platform.system()}
            elif action == 'stop':
                result = {'exitCode': stop(), 'stopped': True}
            else:
                raise ValueError('Unsupported owned control action')
            print(json.dumps(result), flush=True)
    finally:
        stop()
        daemon_log.close()


if __name__ == '__main__':
    main()
