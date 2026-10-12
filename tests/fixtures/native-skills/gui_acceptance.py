#!/usr/bin/env python3
"""Drive native skill selection with OS input and inspect the real model request."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import threading
import time
from http.server import ThreadingHTTPServer

sys.dont_write_bytecode = True


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=Path(__file__).resolve().parents[3])
    parser.add_argument('--artifacts', type=Path, required=True)
    parser.add_argument('--script', type=Path, help='JSON array of OS input actions; otherwise read JSON lines from stdin')
    args = parser.parse_args()
    root, out = args.root.resolve(), args.artifacts.resolve()
    if out == root or root in out.parents:
        parser.error('Artifacts must be outside the repository')
    out.mkdir(parents=True, exist_ok=False)
    peer = load('native_skills_peer', Path(__file__).with_name('acceptance.py'))
    module = load('native_desktop', root / 'tests/fixtures/desktop-gui/acceptance.py')
    class NativeDesktop(module.Desktop):
        def launch_host(self):
            # WebKit's OS sandbox needs the real home containing the isolated XDG paths.
            # Only this GUI process uses it; the daemon and Codex retain the fresh fixture HOME.
            isolated_home = self.env['HOME']
            self.env['HOME'] = os.environ['HOME']
            try:
                return super().launch_host()
            finally:
                self.env['HOME'] = isolated_home

    desktop = NativeDesktop(root, out)
    codex = Path(shutil.which('codex')).resolve(strict=True)
    home = out / 'home'
    config = home / '.codex'
    config.mkdir(parents=True)
    desktop.env = {key: os.environ[key] for key in ['PATH', 'USER', 'LOGNAME', 'LANG', 'XDG_RUNTIME_DIR'] if key in os.environ}
    desktop.env['GDK_BACKEND'] = 'x11'
    desktop.env.update(HOME=str(home), CODEX_HOME=str(config), XDG_CONFIG_HOME=str(home / '.config'), XDG_STATE_HOME=str(home / '.local/state'))
    skills = {}
    for name, marker in peer.MARKERS.items():
        path = desktop.project / '.agents/skills' / name / 'SKILL.md'
        path.parent.mkdir(parents=True)
        path.write_text('---\nname: same-name\ndescription: Deterministic exact-path acceptance skill.\n---\n\nAlways reply exactly: ' + marker + '\n', encoding='utf-8')
        skills[name] = path
    server = ThreadingHTTPServer(('127.0.0.1', 0), peer.Responses)
    server.daemon_threads = True
    server.artifacts, server.receipts = out, []
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    config.joinpath('config.toml').write_text('model = "gpt-5.3-codex"\nmodel_provider = "fixture"\napproval_policy = "never"\nsandbox_mode = "read-only"\n\n[model_providers.fixture]\nname = "Deterministic acceptance peer"\nbase_url = "http://127.0.0.1:' + str(server.server_port) + '/v1"\nwire_api = "responses"\nrequires_openai_auth = false\nsupports_websockets = false\nrequest_max_retries = 0\nstream_max_retries = 0\n', encoding='utf-8')
    owned_native_session = None
    try:
        desktop.start()
        native = desktop.rpc('session/create', {'harness': 'codex', 'cwd': str(desktop.project), 'model': 'gpt-5.3-codex', 'execution': {'configHome': str(config)}, 'config': {'executable': str(codex), 'modelProvider': 'fixture', 'approvalPolicy': 'never', 'sandbox': 'read-only'}})
        desktop.session = native['id']
        owned_native_session = desktop.session
        desktop.rpc('session/metadata', {'sessionId': desktop.session, 'title': 'Native exact skill path'})
        commands = desktop.rpc('agent/commands', {'sessionId': desktop.session})
        listed = [entry for entry in commands if entry['name'] == 'same-name']
        assert {entry['path'] for entry in listed} == {str(path) for path in skills.values()}
        desktop.stop(desktop.host)
        time.sleep(0.5)
        desktop.env['SWARMIE_DESKTOP_URL'] = desktop.url + '/?session=' + desktop.session
        desktop.launch_host()
        ready = {'display': desktop.display, 'window': desktop.window, 'sessionId': desktop.session, 'skills': listed, 'artifacts': str(out)}
        (out / 'ready.json').write_text(json.dumps(ready, indent=2) + '\n', encoding='utf-8')
        print(json.dumps({'ready': ready}), flush=True)
        actions = json.loads(args.script.read_text()) if args.script else (json.loads(line) for line in sys.stdin if line.strip())
        for request in actions:
            try:
                if request['action'] == 'verify-model':
                    selected = request['selected']
                    deadline = time.monotonic() + 60
                    while not server.receipts and time.monotonic() < deadline:
                        time.sleep(0.1)
                    assert server.receipts, 'No model request from the native GUI'
                    assert all(item['loadedInstructionMarkers'][selected] for item in server.receipts)
                    assert all(not item['loadedInstructionMarkers'][other] for item in server.receipts for other in peer.MARKERS if other != selected)
                    assert all(not item['authorizationPresent'] for item in server.receipts)
                    transcript = desktop.rpc('session/transcript', {'sessionId': desktop.session})
                    assert any(event['type'] == 'text.delta' and peer.MARKERS[selected] in event['data']['text'] for event in transcript)
                    result = {'actualInstalledAppServer': True, 'actualNativeGuiSelection': True, 'selectedPath': str(skills[selected]), 'selectedInstructionMarkerOnly': True, 'actualModelRequests': len(server.receipts), 'authorizationPresent': False}
                    (out / 'verified.json').write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8')
                else:
                    result = desktop.action(request)
                response = {'request': request, 'result': result}
            except Exception as error:
                response = {'request': request, 'error': str(error)}
            with (out / 'actions.jsonl').open('a', encoding='utf-8') as output:
                output.write(json.dumps(response) + '\n')
            print(json.dumps(response), flush=True)
    finally:
        if owned_native_session is not None:
            try:
                desktop.rpc('session/close', {'sessionId': owned_native_session})
            except Exception:
                pass
        desktop.close()
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)


if __name__ == '__main__':
    main()
