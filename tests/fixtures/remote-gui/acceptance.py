#!/usr/bin/env python3
"""Drive an authenticated real macOS workspace through a native Linux WebView and production SSH forwarding."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shlex
import signal
import socket
import subprocess
import sys
import time
import urllib.request
import uuid

sys.dont_write_bytecode = True


def module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    return value


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=Path(__file__).resolve().parents[3])
    parser.add_argument('--artifacts', type=Path, required=True)
    parser.add_argument('--mac-source', required=True, help='Absolute path to the current source checkout on the remote host')
    parser.add_argument('--host', required=True, help='Configured SSH alias for the macOS test host')
    parser.add_argument('--remote-artifacts', required=True, help='Absolute external artifact root on the remote host')
    args = parser.parse_args()
    root, out = args.root.resolve(), args.artifacts.resolve()
    if out == root or root in out.parents:
        parser.error('All acceptance state must be outside the repository')
    out.mkdir(parents=True, exist_ok=False)
    desktop = module('native_desktop', root / 'tests/fixtures/desktop-gui/acceptance.py').Desktop(root, out)
    reader = module('remote_acceptance', root / 'tests/fixtures/remote-workspace/acceptance.py')
    vendor = root / 'tests/fixtures/resources-gui/vendor'
    desktop.env.update(PATH=str(vendor / 'usr/bin') + ':' + os.environ['PATH'],
        LD_LIBRARY_PATH=str(vendor / 'usr/lib/x86_64-linux-gnu'),
        TESSDATA_PREFIX=str(vendor / 'usr/share/tesseract-ocr/5/tessdata'),
        GTK_A11Y='none', WEBKIT_DISABLE_COMPOSITING_MODE='1')
    tokens = {'initial': uuid.uuid4().hex + uuid.uuid4().hex, 'replacement': uuid.uuid4().hex + uuid.uuid4().hex}
    private = out / 'private'
    private.mkdir(mode=0o700)
    for name, value in tokens.items():
        path = private / (name + '-token')
        path.write_text(value + '\n')
        path.chmod(0o600)
    active_token = tokens['initial']
    actions, ssh_events, controls = [], [], []
    remote, supervisor = None, None
    unique = uuid.uuid4().hex
    remote_base = args.remote_artifacts.rstrip('/')
    if not remote_base.startswith('/'):
        parser.error('--remote-artifacts must be an absolute remote directory')
    remote_out = remote_base + '/remote-gui-current-' + unique
    control_path = remote_base + '/remote-gui-control-' + unique + '.py'
    bootstrap = 'import json,os,sys; from pathlib import Path; v=json.load(sys.stdin); p=Path(v["path"]); p.parent.mkdir(parents=True,exist_ok=True); fd=os.open(p,os.O_WRONLY|os.O_CREAT|os.O_EXCL,0o600); f=os.fdopen(fd,"w"); f.write(v["source"]); f.close()'
    subprocess.run(['ssh', '-o', 'BatchMode=yes', args.host, shlex.join(['/usr/bin/python3', '-c', bootstrap])],
        input=json.dumps({'path': control_path, 'source': Path(__file__).with_name('remote_host.py').read_text()}), text=True, check=True)

    def persist():
        for name, values in [('actions.json', actions), ('ssh-events.json', ssh_events), ('remote-controls.json', controls)]:
            text = json.dumps(values, indent=2) + '\n'
            assert all(token not in text for token in tokens.values())
            (out / name).write_text(text)

    def control(action, credential=None, **fields):
        remote.stdin.write(json.dumps({'action': action, **({'token': credential} if credential else {}), **fields}) + '\n')
        remote.stdin.flush()
        value = reader.line(remote)
        controls.append({'action': action, 'result': value})
        persist()
        return value

    def event(kind):
        deadline = time.monotonic() + 40
        while time.monotonic() < deadline:
            value = reader.line(supervisor, max(.1, deadline - time.monotonic()))
            ssh_events.append(value)
            persist()
            if value['event'] == kind:
                return value
        raise TimeoutError('Production SSH event ' + kind)

    def rpc(method, params=None):
        request = urllib.request.Request(desktop.url + '/rpc', json.dumps({'jsonrpc': '2.0', 'id': 1,
            'method': method, 'params': params or {}}).encode(), {'Content-Type': 'application/json', 'Authorization': 'Bearer ' + active_token})
        with urllib.request.urlopen(request, timeout=30) as response:
            result = json.load(response)
        if 'error' in result:
            raise RuntimeError(json.dumps(result['error']))
        return result['result']

    desktop.rpc = rpc
    try:
        log = (out / 'remote.log').open('a')
        desktop.logs.append(log)
        remote = subprocess.Popen(['ssh', '-o', 'BatchMode=yes', args.host, shlex.join(['/usr/bin/python3', '-u', control_path,
            args.mac_source + '/target/release/swarmie', remote_out, args.mac_source + '/apps/desktop/dist', remote_base])],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=log, text=True, start_new_session=True)
        with socket.socket() as socket_:
            socket_.bind(('127.0.0.1', 0))
            port = socket_.getsockname()[1]
        desktop.url = f'http://127.0.0.1:{port}'
        started = control('start', active_token, browserOrigin=desktop.url)
        supervisor_log = (out / 'supervisor.log').open('a')
        desktop.logs.append(supervisor_log)
        supervisor = subprocess.Popen([str(root / 'target/debug/swarmie'), '--state-dir', str(out / 'cli-client/sessions'),
            'remote', 'ssh', 'native-gui-fixture', args.host, '--local-port', str(port), '--remote-port', str(started['port']), '--reconnect'],
            env={**os.environ, 'SWARMIE_TOKEN': active_token}, stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE, stderr=supervisor_log, text=True, start_new_session=True)
        desktop.children.append(supervisor)
        connected = event('ssh.connected')
        assert connected['authenticated']
        xvfb = desktop.child(['Xvfb', '-displayfd', '1', '-screen', '0', '1600x1100x24', '-nolisten', 'tcp'], pipe=True)
        desktop.display = ':' + desktop.ready(xvfb, '')
        desktop.env.update(DISPLAY=desktop.display, SWARMIE_DESKTOP_URL=desktop.url + '/?project=' + started['project'],
            SWARMIE_DESKTOP_MANAGED='1', XDG_DATA_HOME=str(out / 'xdg-data'), XDG_CACHE_HOME=str(out / 'xdg-cache'))
        (out / 'dbus.conf').write_text('<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN" "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd"><busconfig><type>session</type><listen>unix:tmpdir=/tmp</listen><policy context="default"><allow send_destination="*"/><allow receive_sender="*"/><allow own="*"/></policy></busconfig>')
        desktop.launch_host()
        desktop.x('windowsize', desktop.window, '1400', '1000')
        ready = {'remote': started, 'url': desktop.url, 'window': desktop.window, 'display': desktop.display,
            'remoteArtifacts': remote_out, 'forwardAuthenticated': True, 'nativeClientInitiallyAuthenticated': False,
            'initialTokenSha256': hashlib.sha256(active_token.encode()).hexdigest()}
        (out / 'ready.json').write_text(json.dumps(ready, indent=2) + '\n')
        print(json.dumps({'ready': ready}), flush=True)
        for raw in sys.stdin:
            request = json.loads(raw)
            try:
                action = request['action']
                if action in ('type', 'token-type'):
                    text = tokens[request.get('generation', 'initial')] if action == 'token-type' else request['text']
                    desktop.x('windowfocus', '--sync', desktop.window)
                    subprocess.run(['xdotool', 'type', '--clearmodifiers', '--delay', '20', '--file', '-'],
                        input=text, text=True, env=desktop.env, check=True, capture_output=True)
                    time.sleep(.5)
                    result = {'typedCharacters': len(text), 'secret': action == 'token-type'}
                elif action == 'capture':
                    result = desktop.capture(request['name'])
                    image = Path(result['screenshot'])
                    ocr = subprocess.check_output([str(vendor / 'usr/bin/tesseract'), str(image), 'stdout'], env=desktop.env,
                        text=True, stderr=subprocess.DEVNULL)
                    image.with_suffix('.txt').write_text(ocr)
                    result['ocr'] = ocr
                elif action == 'disconnect':
                    os.kill(connected['sshPid'], signal.SIGTERM)
                    disconnected = event('ssh.disconnected')
                    event('ssh.reconnecting')
                    connected = event('ssh.connected')
                    result = {'disconnected': disconnected, 'connected': connected}
                elif action == 'rotate':
                    active_token = tokens['replacement']
                    result = control('rotate', active_token)
                elif action == 'remote-verify':
                    result = control('verify')
                elif action == 'devices':
                    result = control('devices')
                elif action == 'owned-device':
                    result = control('owned-device', deviceId=request['deviceId'])
                elif action == 'repair-cli':
                    command = [str(root / 'target/debug/swarmie'), '--state-dir', str(out / 'cli-client/sessions'),
                        'remote', 'pair', 'native-gui-fixture', desktop.url + '/rpc']
                    completed = subprocess.run(command, env={**desktop.env, 'SWARMIE_TOKEN': active_token}, capture_output=True, text=True, check=True)
                    result = json.loads(completed.stdout)
                else:
                    result = desktop.action(request)
                response = {'request': request, 'result': result}
            except Exception as error:
                response = {'request': request, 'error': str(error)}
            text = json.dumps(response)
            assert all(token not in text for token in tokens.values())
            actions.append(response)
            persist()
            print(text, flush=True)
    finally:
        desktop.close()
        if supervisor and supervisor.poll() is None:
            supervisor.send_signal(signal.SIGINT)
            supervisor.wait(timeout=10)
        if remote and remote.poll() is None:
            remote.stdin.close()
            try:
                remote.wait(timeout=25)
            except subprocess.TimeoutExpired:
                os.killpg(remote.pid, signal.SIGTERM)
                remote.wait(timeout=10)
        (out / 'remote-cleanup.json').write_text(json.dumps({'remoteControllerExited': remote is None or remote.poll() is not None,
            'supervisorExited': supervisor is None or supervisor.poll() is not None, 'remoteArtifacts': remote_out}, indent=2) + '\n')
        persist()


if __name__ == '__main__':
    main()
