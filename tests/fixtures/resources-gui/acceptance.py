#!/usr/bin/env python3
"""Interactive OS-only native GUI resources acceptance with an actual local Codex provider."""
import argparse
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import threading
import time
import urllib.parse
import uuid

sys.dont_write_bytecode = True


class Responses(BaseHTTPRequestHandler):
    protocol_version = 'HTTP/1.1'

    def log_message(self, *_):
        pass

    def do_POST(self):
        size = int(self.headers['Content-Length'])
        assert size < 8 * 1024 * 1024
        body = json.loads(self.rfile.read(size))
        authorization = self.headers.get('Authorization')
        self.server.receipts.append({'path': self.path, 'model': body.get('model'),
            'authorizationPresent': authorization is not None,
            'authorizationSha256': hashlib.sha256(authorization.encode()).hexdigest() if authorization else None,
            'input': body.get('input')})
        (self.server.out / 'model-requests.json').write_text(json.dumps(self.server.receipts, indent=2) + '\n')
        text = 'GUI_NATIVE_DEFAULTS_OK'
        response_id, item_id = 'resp_' + uuid.uuid4().hex, 'msg_' + uuid.uuid4().hex
        part = {'type': 'output_text', 'text': text, 'annotations': []}
        item = {'id': item_id, 'type': 'message', 'role': 'assistant', 'status': 'completed', 'content': [part]}
        self.send_response(200)
        self.send_header('Content-Type', 'text/event-stream')
        self.send_header('Connection', 'close')
        self.end_headers()

        def emit(kind, **values):
            self.wfile.write(('event: ' + kind + '\ndata: ' + json.dumps({'type': kind, **values}) + '\n\n').encode())
            self.wfile.flush()

        emit('response.created', response={'id': response_id, 'object': 'response', 'status': 'in_progress', 'output': []})
        emit('response.output_item.added', output_index=0, item={**item, 'status': 'in_progress', 'content': []})
        emit('response.content_part.added', item_id=item_id, output_index=0, content_index=0, part={**part, 'text': ''})
        emit('response.output_text.delta', item_id=item_id, output_index=0, content_index=0, delta=text)
        emit('response.output_text.done', item_id=item_id, output_index=0, content_index=0, text=text)
        emit('response.content_part.done', item_id=item_id, output_index=0, content_index=0, part=part)
        emit('response.output_item.done', output_index=0, item=item)
        emit('response.completed', response={'id': response_id, 'object': 'response', 'status': 'completed', 'output': [item],
            'usage': {'input_tokens': 100, 'output_tokens': 10, 'total_tokens': 110}})
        self.close_connection = True


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=Path(__file__).resolve().parents[3])
    parser.add_argument('--artifacts', type=Path, required=True)
    args = parser.parse_args()
    root, out = args.root.resolve(), args.artifacts.resolve()
    if out == root or root in out.parents:
        parser.error('Artifacts must be outside the repository')
    out.mkdir(parents=True, exist_ok=False)
    codex = Path(shutil.which('codex')).resolve()
    spec = importlib.util.spec_from_file_location('desktop_fixture', root / 'tests/fixtures/desktop-gui/acceptance.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    desktop = module.Desktop(root, out)
    vendor = Path(__file__).with_name('vendor').resolve()
    desktop.env.update(PATH=str(vendor / 'usr/bin') + ':' + os.environ['PATH'],
                       LD_LIBRARY_PATH=str(vendor / 'usr/lib/x86_64-linux-gnu'),
                       TESSDATA_PREFIX=str(vendor / 'usr/share/tesseract-ocr/5/tessdata'),
                       WEBKIT_DISABLE_COMPOSITING_MODE='1', GTK_A11Y='none')
    config = out / 'codex-config'
    config.mkdir(mode=0o700)
    desktop.env['CODEX_HOME'] = str(config)
    secret = 'synthetic-gui-fixture-' + uuid.uuid4().hex
    private = out / 'private'
    private.mkdir(mode=0o700)
    secret_path = private / 'synthetic-secret'
    secret_path.write_text(secret)
    secret_path.chmod(0o600)
    desktop.env["GUI_FIXTURE_API_KEY"] = secret
    server = ThreadingHTTPServer(('127.0.0.1', 0), Responses)
    server.daemon_threads = True
    server.out, server.receipts = out, []
    threading.Thread(target=server.serve_forever, daemon=True).start()
    (config / 'config.toml').write_text('model = "gpt-5.3-codex"\nmodel_provider = "fixture"\napproval_policy = "never"\nsandbox_mode = "read-only"\n\n[model_providers.fixture]\nname = "Controlled native GUI provider"\nbase_url = "http://127.0.0.1:1/v1"\nwire_api = "responses"\nenv_key = "GUI_FIXTURE_API_KEY"\nrequires_openai_auth = false\nsupports_websockets = false\nrequest_max_retries = 0\nstream_max_retries = 0\n')
    for command in [['git', 'init', '-b', 'main', str(desktop.project)], ['git', '-C', str(desktop.project), 'add', '.'],
        ['git', '-C', str(desktop.project), '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid', 'commit', '-m', 'Initialize controlled GUI project']]:
        subprocess.run(command, check=True, capture_output=True)
    actions = []
    fingerprints = {str(path.relative_to(root)): hashlib.sha256(path.read_bytes()).hexdigest() for path in
        [root / 'target/debug/mpai', root / 'apps/desktop/zig-out/bin/mpai-desktop', *sorted((root / 'apps/desktop/dist').rglob('*.js'))]}
    (out / 'build.json').write_text(json.dumps(fingerprints, indent=2) + '\n')
    try:
        assets = out / 'desktop-assets'
        shutil.copytree(root / 'apps/desktop/dist', assets)
        daemon = desktop.child([str(root / 'target/debug/mpai'), '--state-dir', str(out / 'sessions'), 'serve', '--http',
            '127.0.0.1:0', '--desktop-assets', str(assets), '--workbench', '--git-worktrees', str(out / 'worktrees'),
            '--resources-root', str(out / 'resources')], pipe=True)
        desktop.url = desktop.ready(daemon, 'HTTP listening on ').removesuffix('/rpc')
        desktop.rpc('runtime/initialize')
        opened = desktop.rpc('extension/invoke', {'extensionId': 'git', 'command': 'project.open', 'arguments': {'path': str(desktop.project)}})
        worktree = desktop.rpc('extension/invoke', {'extensionId': 'git', 'command': 'worktree.create', 'arguments': {'projectId': opened['id'], 'branch': 'gui-defaults'}})
        xvfb = desktop.child(['Xvfb', '-displayfd', '1', '-screen', '0', '1600x1100x24', '-nolisten', 'tcp'], pipe=True)
        desktop.display = ':' + desktop.ready(xvfb, '')
        desktop.env.update(DISPLAY=desktop.display, MPAI_DESKTOP_URL=desktop.url + '/?project=' + urllib.parse.quote(str(desktop.project)),
            MPAI_DESKTOP_MANAGED='1', XDG_DATA_HOME=str(out / 'xdg-data'), XDG_CACHE_HOME=str(out / 'xdg-cache'))
        (out / 'dbus.conf').write_text('<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN" "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd"><busconfig><type>session</type><listen>unix:tmpdir=/tmp</listen><policy context="default"><allow send_destination="*"/><allow receive_sender="*"/><allow own="*"/></policy></busconfig>')
        desktop.launch_host()
        desktop.x('windowsize', desktop.window, '1400', '1000')
        native_config = {'executable': str(codex), 'modelProvider': 'fixture', 'approvalPolicy': 'never', 'sandbox': 'read-only',
            'config': {'model_providers.fixture.base_url': 'http://127.0.0.1:' + str(server.server_port) + '/v1'}}
        print(json.dumps({'ready': {'url': desktop.url, 'project': str(desktop.project), 'worktree': worktree,
            'configHome': str(config), 'nativeConfig': native_config, 'credentialId': 'gui-provider-key',
            'credentialEnvKey': 'GUI_FIXTURE_API_KEY', 'expectedAuthorizationSha256': hashlib.sha256(('Bearer ' + secret).encode()).hexdigest(),
            'codexVersion': subprocess.check_output([str(codex), '--version'], text=True).strip(), 'window': desktop.window}}), flush=True)
        for raw in sys.stdin:
            request = json.loads(raw)
            safe = dict(request)
            try:
                if request['action'] in ['type', 'secret-type']:
                    value = secret if request['action'] == 'secret-type' else request['text']
                    desktop.x('windowfocus', '--sync', desktop.window)
                    subprocess.run(['xdotool', 'type', '--clearmodifiers', '--delay', '20', '--file', '-'],
                        input=value, text=True, env=desktop.env, check=True, capture_output=True)
                    time.sleep(.5)
                    result = {'typedCharacters': len(value), 'secret': request['action'] == 'secret-type'}
                    if request['action'] == 'secret-type':
                        safe = {'action': 'secret-type'}
                elif request['action'] == 'capture':
                    time.sleep(.4)
                    geometry = dict(line.split('=', 1) for line in desktop.x('getwindowgeometry', '--shell', desktop.window).splitlines() if '=' in line)
                    path = out / (request['name'] + '.png')
                    subprocess.run(['ffmpeg', '-nostdin', '-loglevel', 'error', '-y', '-f', 'x11grab', '-video_size',
                        geometry['WIDTH'] + 'x' + geometry['HEIGHT'], '-i', desktop.display + '+' + geometry['X'] + ',' + geometry['Y'], '-frames:v', '1', str(path)], env=desktop.env, check=True)
                    ocr = subprocess.check_output([str(vendor / 'usr/bin/tesseract'), str(path), 'stdout'], env=desktop.env, text=True, stderr=subprocess.DEVNULL)
                    (out / (request['name'] + '.txt')).write_text(ocr)
                    result = {'screenshot': str(path), 'ocr': ocr}
                elif request['action'] == 'inspect-private-storage':
                    desktop.stop(desktop.host)
                    allowed = {secret_path, out / 'resources/credentials.json'}
                    leaks, native_files = [], []
                    for path in out.rglob('*'):
                        if path.is_file() and path not in allowed:
                            data = path.read_bytes()
                            if secret.encode() in data or secret.encode('utf-16le') in data:
                                (native_files if config in path.parents else leaks).append(str(path.relative_to(out)))
                    result = {'unexpectedPublicPlaintextFiles': leaks, 'nativeConfigPlaintextFiles': native_files,
                        'nativeConfigHomeMode': oct(config.stat().st_mode & 0o777),
                        'allowedCredentialFileMode': oct((out / 'resources/credentials.json').stat().st_mode & 0o777)}
                    desktop.launch_host()
                else:
                    result = desktop.action(request)
                response = {'request': safe, 'result': result}
            except Exception as error:
                response = {'request': safe, 'error': str(error)}
            serialized = json.dumps(response)
            assert secret not in serialized
            actions.append(response)
            (out / 'actions.json').write_text(json.dumps(actions, indent=2) + '\n')
            print(serialized, flush=True)
    finally:
        desktop.close()
        server.shutdown()
        server.server_close()
        (out / 'model-requests.json').write_text(json.dumps(server.receipts, indent=2) + '\n')


if __name__ == '__main__':
    main()
