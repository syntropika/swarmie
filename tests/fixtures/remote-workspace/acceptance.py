#!/usr/bin/env python3
"""Verify authenticated CLI clients against a real macOS workspace through actual SSH forwarding."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import queue
import shlex
import signal
import socket
import subprocess
import threading
import time
import urllib.error
import urllib.request
import uuid

READERS = {}


def line(process, timeout=35):
    if process not in READERS:
        items = queue.Queue()
        READERS[process] = items
        def read():
            for value in process.stdout:
                items.put(value)
            items.put(None)
        threading.Thread(target=read, daemon=True).start()
    try:
        value = READERS[process].get(timeout=timeout)
    except queue.Empty:
        raise TimeoutError('Owned process receipt') from None
    if not value:
        raise RuntimeError('Owned process exited without receipt')
    return json.loads(value)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=Path(__file__).resolve().parents[3])
    parser.add_argument('--artifacts', type=Path, required=True)
    parser.add_argument('--mac-binary', required=True)
    parser.add_argument('--host', required=True, help='Configured SSH alias for the macOS test host')
    parser.add_argument('--remote-artifacts', required=True, help='Absolute external artifact root on the remote host')
    args = parser.parse_args()
    root, out = args.root.resolve(), args.artifacts.resolve()
    if out == root or root in out.parents:
        parser.error('Artifacts must be outside the repository')
    out.mkdir(parents=True, exist_ok=False)
    token = uuid.uuid4().hex + uuid.uuid4().hex
    replacement = uuid.uuid4().hex + uuid.uuid4().hex
    private = out / 'private'
    private.mkdir(mode=0o700)
    for name, value in [('initial-token', token), ('replacement-token', replacement)]:
        file = private / name
        file.write_text(value + '\n')
        file.chmod(0o600)
    unique = uuid.uuid4().hex
    remote_base = args.remote_artifacts.rstrip('/')
    if not remote_base.startswith('/'):
        parser.error('--remote-artifacts must be an absolute remote directory')
    control_path = remote_base + '/remote-workspace-control-' + unique + '.py'
    remote_out = remote_base + '/remote-workspace-current-' + unique
    control_source = Path(__file__).with_name('remote_host.py').read_text()
    bootstrap = 'import json,os,sys; from pathlib import Path; v=json.load(sys.stdin); p=Path(v["path"]); p.parent.mkdir(parents=True,exist_ok=True); fd=os.open(p,os.O_WRONLY|os.O_CREAT|os.O_EXCL,0o600); f=os.fdopen(fd,"w"); f.write(v["source"]); f.close()'
    subprocess.run(['ssh', '-o', 'BatchMode=yes', args.host, shlex.join(['/usr/bin/python3', '-c', bootstrap])],
                   input=json.dumps({'path': control_path, 'source': control_source}), text=True, check=True)
    logs = []
    def log(name):
        file = (out / name).open('a')
        logs.append(file)
        return file
    remote = subprocess.Popen(['ssh', '-o', 'BatchMode=yes', args.host,
        shlex.join(['/usr/bin/python3', '-u', control_path, args.mac_binary, remote_out, remote_base])],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=log('remote.log'), text=True, start_new_session=True)
    supervisor = None
    receipts, events = [], []
    report = {'checks': {}, 'host': args.host, 'nativeGuiTested': False,
              'localDaemonSha256': hashlib.sha256((root / 'target/debug/mpai').read_bytes()).hexdigest()}
    clients = [out / 'client-one', out / 'client-two']
    for path in clients:
        path.mkdir(mode=0o700)
    env = os.environ.copy()
    env.pop('MPAI_TOKEN', None)

    def control(action, credential=None):
        remote.stdin.write(json.dumps({'action': action, **({'token': credential} if credential else {})}) + '\n')
        remote.stdin.flush()
        value = line(remote)
        receipts.append({'remoteControl': action, 'response': value})
        return value

    def cli(client, command, credential=None, fail=False):
        result = subprocess.run([str(root / 'target/debug/mpai'), '--state-dir', str(client / 'sessions'), *command],
            env=dict(env, **({'MPAI_TOKEN': credential} if credential else {})), text=True, capture_output=True, timeout=40)
        receipt = {'client': client.name, 'command': command, 'exitCode': result.returncode,
                   'stdout': result.stdout, 'stderr': result.stderr}
        assert token not in json.dumps(receipt) and replacement not in json.dumps(receipt)
        receipts.append(receipt)
        if fail:
            assert result.returncode != 0, receipt
            return receipt
        assert result.returncode == 0, receipt
        return json.loads(result.stdout) if result.stdout.strip() else None

    def call(client, method, params=None, fail=False):
        return cli(client, ['remote', 'call', 'remote-fixture', method, '--params', json.dumps(params or {})], fail=fail)

    def invoke(client, command, arguments=None):
        return call(client, 'extension/invoke', {'extensionId': 'workspace', 'command': command, 'arguments': arguments or {}})

    def event(kind):
        deadline = time.monotonic() + 40
        while time.monotonic() < deadline:
            value = line(supervisor, max(.1, deadline - time.monotonic()))
            assert token not in json.dumps(value) and replacement not in json.dumps(value)
            events.append(value)
            if value['event'] == kind:
                return value
        raise TimeoutError(kind)

    def unauthorized(credential=None):
        request = urllib.request.Request(url, json.dumps({'jsonrpc': '2.0', 'id': 1, 'method': 'runtime/initialize', 'params': {}}).encode(),
            {'Content-Type': 'application/json', **({'Authorization': 'Bearer ' + credential} if credential else {})})
        try:
            urllib.request.urlopen(request, timeout=10)
            raise AssertionError('Unauthorized request succeeded')
        except urllib.error.HTTPError as error:
            assert error.code == 401, error.code
            return error.code

    try:
        started = control('start', token)
        assert started['os'] == 'Darwin' and started['tokenFileMode'] == '0o600', started
        report['remote'] = started
        with socket.socket() as socket_:
            socket_.bind(('127.0.0.1', 0))
            port = socket_.getsockname()[1]
        url = f'http://127.0.0.1:{port}/rpc'
        supervisor = subprocess.Popen([str(root / 'target/debug/mpai'), '--state-dir', str(clients[0] / 'sessions'),
            'remote', 'ssh', 'remote-fixture', args.host, '--local-port', str(port), '--remote-port', str(started['port']), '--reconnect'],
            env=dict(env, MPAI_TOKEN=token), stdout=subprocess.PIPE, stderr=log('supervisor.log'), text=True, start_new_session=True)
        first = event('ssh.connected')
        assert first['authenticated'] is True
        assert unauthorized() == 401 and unauthorized('wrong-fixture-token') == 401
        cli(clients[1], ['remote', 'pair', 'remote-fixture', url], token)
        for client in clients:
            listed = cli(client, ['remote', 'list'])
            assert listed[0]['authenticated'] is True, listed
            registry = client / 'connections.json'
            assert registry.stat().st_mode & 0o777 == 0o600
        report['checks']['actualAuthenticatedSshForwardTwoClients'] = True
        project = invoke(clients[0], 'project.open', {'path': started['project']})
        assert project['path'] == started['project']
        assert invoke(clients[1], 'files.read', {'projectId': project['id'], 'path': 'hello.txt'})['content'] == 'REMOTE_FILE_BASELINE\n'
        invoke(clients[1], 'files.write', {'projectId': project['id'], 'path': 'api-written.txt', 'content': 'REMOTE_API_WRITE_CONFIRMED\n'})
        terminal = invoke(clients[0], 'terminal.start', {'projectId': project['id'], 'program': '/bin/sh', 'args': [], 'cols': 90, 'rows': 24})
        terminal_id = terminal['terminalId']
        invoke(clients[1], 'terminal.write', {'terminalId': terminal_id, 'data':
            "pwd\nuname -s\ncat hello.txt api-written.txt\nprintf 'REMOTE_TERMINAL_WRITE_CONFIRMED\\n' > terminal-written.txt\nprintf 'REMOTE_PTY_DONE\\n'\n"})
        output = ''
        offset = 0
        end = time.monotonic() + 30
        while time.monotonic() < end:
            result = invoke(clients[0], 'terminal.read', {'terminalId': terminal_id, 'after': offset})
            output += result['text']
            offset = result['next']
            if '\r\nREMOTE_PTY_DONE\r\n' in output:
                break
            time.sleep(.08)
        assert started['project'] in output and '\r\nDarwin\r\n' in output and 'REMOTE_FILE_BASELINE' in output and 'REMOTE_API_WRITE_CONFIRMED' in output, output
        assert invoke(clients[1], 'files.read', {'projectId': project['id'], 'path': 'terminal-written.txt'})['content'] == 'REMOTE_TERMINAL_WRITE_CONFIRMED\n'
        direct = control('verify')
        assert direct == {'apiWritten': 'REMOTE_API_WRITE_CONFIRMED\n', 'terminalWritten': 'REMOTE_TERMINAL_WRITE_CONFIRMED\n', 'os': 'Darwin'}
        report['terminalTranscript'] = output
        report['checks']['trueRemoteCwdFilesAndInteractivePty'] = True
        os.kill(first['sshPid'], signal.SIGTERM)
        event('ssh.disconnected')
        event('ssh.reconnecting')
        recovered = event('ssh.connected')
        assert recovered['sshPid'] != first['sshPid'] and recovered['attempt'] > first['attempt']
        assert invoke(clients[1], 'files.read', {'projectId': project['id'], 'path': 'terminal-written.txt'})['content'] == 'REMOTE_TERMINAL_WRITE_CONFIRMED\n'
        invoke(clients[1], 'terminal.write', {'terminalId': terminal_id, 'data': "printf 'REMOTE_PTY_AFTER_RECONNECT\\n'\n"})
        after_reconnect = ''
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            result = invoke(clients[0], 'terminal.read', {'terminalId': terminal_id, 'after': offset})
            after_reconnect += result['text']
            offset = result['next']
            if '\r\nREMOTE_PTY_AFTER_RECONNECT\r\n' in after_reconnect:
                break
            time.sleep(.08)
        assert '\r\nREMOTE_PTY_AFTER_RECONNECT\r\n' in after_reconnect, after_reconnect
        report['terminalAfterReconnect'] = after_reconnect
        report['checks']['realSshReconnectSameRemoteWorkspace'] = True
        invoke(clients[0], 'terminal.close', {'terminalId': terminal_id})
        rotated = control('rotate', replacement)
        assert rotated['port'] == started['port'] and rotated['pid'] != started['pid']
        assert unauthorized(token) == 401 and unauthorized() == 401
        call(clients[0], 'runtime/initialize', fail=True)
        call(clients[1], 'runtime/initialize', fail=True)
        cli(clients[1], ['remote', 'pair', 'remote-fixture', url], replacement)
        assert call(clients[1], 'runtime/initialize')['name'] == 'mpai'
        project = invoke(clients[1], 'project.open', {'path': started['project']})
        assert invoke(clients[1], 'files.read', {'projectId': project['id'], 'path': 'api-written.txt'})['content'] == 'REMOTE_API_WRITE_CONFIRMED\n'
        report['checks']['tokenRevocationAndExplicitRePair'] = True
        cli(clients[1], ['remote', 'remove', 'remote-fixture'])
        assert cli(clients[1], ['remote', 'list']) == []
        call(clients[1], 'runtime/initialize', fail=True)
        report['checks']['connectionRemovalLifecycle'] = True
        supervisor.send_signal(signal.SIGINT)
        event('ssh.stopped')
        assert supervisor.wait(timeout=10) == 0
        stopped = control('stop')
        assert stopped['exitCode'] == 0
        report['checks']['ownedRemoteDaemonAndForwardStopped'] = True
    except Exception as error:
        report['failure'] = repr(error)
        raise
    finally:
        if supervisor and supervisor.poll() is None:
            supervisor.send_signal(signal.SIGINT)
            try:
                supervisor.wait(timeout=10)
            except subprocess.TimeoutExpired:
                os.killpg(supervisor.pid, signal.SIGKILL)
                supervisor.wait()
        if remote.poll() is None:
            remote.stdin.close()
            try:
                remote.wait(timeout=20)
            except subprocess.TimeoutExpired:
                os.killpg(remote.pid, signal.SIGTERM)
                remote.wait(timeout=10)
        report['checks']['ownedLocalControllerProcessesExited'] = remote.poll() is not None and (supervisor is None or supervisor.poll() is not None)
        for file in logs:
            file.close()
        report['remoteArtifacts'] = remote_out
        for name, value in [('result.json', report), ('receipts.json', receipts), ('ssh-events.json', events)]:
            serialized = json.dumps(value, indent=2) + '\n'
            assert token not in serialized and replacement not in serialized
            (out / name).write_text(serialized)
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
