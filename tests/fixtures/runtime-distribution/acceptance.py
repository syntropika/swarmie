#!/usr/bin/env python3
"""Qualify a real Linux runtime package and service command with an isolated manager fixture."""
import argparse
import json
import os
from pathlib import Path
import shlex
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.request
import uuid

MANAGER = '''#!/usr/bin/env python3
import json, os, pathlib, signal, sys
root = pathlib.Path(os.environ['SWARMIE_MANAGER_FIXTURE'])
args = sys.argv[1:]
with (root / 'manager-calls.jsonl').open('a') as file:
    file.write(json.dumps(args) + '\\n')
action = args[1]
active = root / 'active'
enabled = root / 'enabled'
if action == 'is-active':
    if (root / 'manager-unavailable').exists():
        print('Manager unavailable', file=sys.stderr); sys.exit(1)
    print('active' if active.exists() else 'inactive')
    sys.exit(0 if active.exists() else 3)
if action == 'start': active.touch()
elif action == 'stop':
    if (root / 'pid').exists(): os.kill(int((root / 'pid').read_text()), signal.SIGINT)
    active.unlink(missing_ok=True)
elif action == 'enable': enabled.touch()
elif action == 'disable': enabled.unlink(missing_ok=True)
elif action == 'status': sys.exit(0 if active.exists() else 3)
elif action != 'daemon-reload': sys.exit(2)
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--package', required=True, type=Path)
    parser.add_argument('--artifacts', required=True, type=Path)
    args = parser.parse_args()
    package = args.package.resolve()
    root = args.artifacts.resolve()
    root.mkdir(parents=True, exist_ok=False)
    receipts = []
    fixture = root / 'manager'
    fixture.mkdir()
    manager_bin = root / 'manager-bin'
    manager_bin.mkdir()
    (manager_bin / 'systemctl').write_text(MANAGER)
    (manager_bin / 'systemctl').chmod(0o755)
    home = root / 'home'
    home.mkdir()
    env = dict(os.environ, HOME=str(home), XDG_CONFIG_HOME=str(home / '.config'),
               PATH=str(manager_bin) + os.pathsep + os.environ['PATH'], SWARMIE_MANAGER_FIXTURE=str(fixture))
    prefix = root / 'installation'
    bin_dir = root / 'bin'
    cli = bin_dir / 'swarmie'
    # Socket paths have platform length limits; artifact roots can be arbitrarily deep.
    state_directory = tempfile.TemporaryDirectory(prefix='swarmie-runtime-state-')
    state = Path(state_directory.name)
    work = root / 'project'
    work.mkdir()
    (work / 'owned.txt').write_text('User-owned working directory.\n')

    def run(command, success=True):
        result = subprocess.run([str(part) for part in command], env=env, capture_output=True, text=True, timeout=60)
        # Never persist the service bearer token.
        receipts.append({'command': [str(part) for part in command], 'exitCode': result.returncode,
                         'stdout': result.stdout, 'stderr': result.stderr})
        (root / 'commands.json').write_text(json.dumps(receipts, indent=2) + '\n')
        if (result.returncode == 0) != success:
            raise RuntimeError(result.stderr or result.stdout)
        return result

    assert sorted(str(path.relative_to(package)) for path in package.rglob('*') if path.is_file()) == [
        'LICENSE', 'NOTICE', 'bin/swarmie', 'install.py', 'runtime-package.json']
    assert 'Apache License' in (package / 'LICENSE').read_text()
    assert 'Copyright 2026 Syntropika' in (package / 'NOTICE').read_text()
    installer = ['python3', package / 'install.py', '--package', package, '--prefix', prefix, '--bin-dir', bin_dir]
    run(installer)
    assert run([cli, '--version']).stdout.strip() == 'swarmie ' + json.loads((package / 'runtime-package.json').read_text())['version']
    (prefix / 'user-data.txt').write_text('Preserve even unexpected files in the installation.\n')
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0))
        port = listener.getsockname()[1]
    service = [cli, '--state-dir', state, 'service']
    installed = json.loads(run(service + ['install', '--port', port]).stdout)
    token_file = Path(installed['tokenFile'])
    token = token_file.read_text().strip()
    assert token_file.stat().st_mode & 0o777 == 0o600
    unit_path = Path(installed['unit'])
    unit = unit_path.read_text()
    assert '--desktop-assets' not in unit and '--state-dir' in unit and 'KillSignal=SIGINT' in unit
    assert not (fixture / 'active').exists()
    run(service + ['enable'])
    assert (fixture / 'enabled').exists() and not (fixture / 'active').exists()
    mock = root / 'script.json'
    mock.write_text(json.dumps({'turns': [{'steps': [
        {'type': 'sleep', 'milliseconds': 5000}, {'type': 'text', 'text': 'completed after runtime update'},
        {'type': 'end', 'outcome': 'completed'}]}]}))
    exec_line = next(line[len('ExecStart='):] for line in unit.splitlines() if line.startswith('ExecStart='))
    command = shlex.split(exec_line) + ['--mock-script', str(mock)]
    process = None
    rpc_id = 0

    def rpc(method, params=None):
        nonlocal rpc_id
        rpc_id += 1
        request = urllib.request.Request(f'http://127.0.0.1:{port}/rpc', data=json.dumps({
            'jsonrpc': '2.0', 'id': rpc_id, 'method': method, 'params': params or {}}).encode(),
            headers={'Content-Type': 'application/json', 'Authorization': 'Bearer ' + token})
        with urllib.request.urlopen(request, timeout=5) as response:
            value = json.load(response)
        if 'error' in value:
            raise RuntimeError(json.dumps(value['error']))
        return value['result']

    def launch():
        nonlocal process
        process = subprocess.Popen(command, env=env, stdout=(root / 'runtime.stdout.log').open('ab'),
                                   stderr=(root / 'runtime.stderr.log').open('ab'))
        (fixture / 'pid').write_text(str(process.pid))
        run(service + ['start'])
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            if process.poll() is not None:
                raise RuntimeError((root / 'runtime.stderr.log').read_text())
            try:
                return rpc('runtime/initialize')['runtimeIdentity']
            except (OSError, urllib.error.URLError):
                time.sleep(0.05)
        raise TimeoutError('Runtime did not become reachable.')

    try:
        identity = launch()
        run(service + ['status'])
        assert 'service is running' in run(service + ['uninstall'], success=False).stderr
        run(installer + ['--uninstall'], success=False)
        assert unit_path.exists() and token_file.read_text().strip() == token
        directory = rpc('directory/register', {'path': str(work)})
        project = rpc('project/create', {'name': 'Distribution qualification',
                                        'directory': {'id': directory['id'], 'revision': directory['revision']}})
        session = rpc('session/create', {'harness': 'mock', 'cwd': str(work)})
        session_id = session['id']
        rpc('session/prompt', {'sessionId': session_id, 'text': 'Finish across installation replacement',
                               'idempotencyKey': str(uuid.uuid4())})
        assert rpc('session/get', {'sessionId': session_id})['state'] == 'running'
        inode = (prefix / 'bin/swarmie').stat().st_ino
        updated = json.loads(run(installer).stdout)
        assert updated['updated'] and updated['restartRequiredForRunningProcesses'] and not updated['processesStopped']
        assert (prefix / 'bin/swarmie').stat().st_ino != inode
        assert process.poll() is None and rpc('runtime/initialize')['runtimeIdentity'] == identity
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            events = rpc('session/events', {'sessionId': session_id, 'afterSequence': 0})
            if any(event['type'] == 'turn.ended' for event in events):
                assert any(event['type'] == 'text.delta' and 'completed after runtime update' in event['data']['text'] for event in events)
                break
            time.sleep(0.05)
        else:
            raise TimeoutError('Running work did not complete after installation replacement.')
        run(service + ['stop'])
        process.wait(timeout=15)
        (fixture / 'manager-unavailable').touch()
        run(service + ['uninstall'], success=False)
        assert unit_path.exists()
        (fixture / 'manager-unavailable').unlink()
        restarted = launch()
        assert restarted['identity'] == identity['identity'] and restarted['incarnationId'] != identity['incarnationId']
        assert any(item['id'] == project['id'] for item in rpc('project/list'))
        assert any(item['id'] == session_id for item in rpc('session/list'))
        run(service + ['stop'])
        process.wait(timeout=15)
        run(service + ['uninstall'])
        assert not unit_path.exists() and not (fixture / 'enabled').exists()
        assert token_file.read_text().strip() == token
        run(installer + ['--uninstall'])
        assert (prefix / 'user-data.txt').is_file() and (work / 'owned.txt').is_file()
        assert state.exists() and not cli.is_symlink()
        result = {'passed': True, 'runtimeOnlyPackage': True, 'realRuntime': True,
                  'activeTurnSurvivedUpdate': True, 'retainedStateSurvivedRestart': True,
                  'tokenAndUserFilesPreserved': True, 'activeServiceUninstallRefused': True,
                  'managerFailurePreservedUnit': True, 'nativeServiceManager': 'isolated fixture',
                  'desktopAssetsRequired': False}
        (root / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
        print(json.dumps(result, indent=2))
    finally:
        if process is not None and process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
        state_directory.cleanup()


if __name__ == '__main__':
    main()
