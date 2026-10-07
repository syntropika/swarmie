#!/usr/bin/env python3
"""Exercise installed Codex skill-path loading through mpai and a local Responses peer."""
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
import uuid

sys.dont_write_bytecode = True
MARKERS = {'first': 'MPAI_EXACT_SKILL_FIRST_20261003', 'second': 'MPAI_EXACT_SKILL_SECOND_20261003'}


def text_values(value):
    if isinstance(value, str):
        yield value
    elif isinstance(value, dict):
        for nested in value.values():
            yield from text_values(nested)
    elif isinstance(value, list):
        for nested in value:
            yield from text_values(nested)


class Responses(BaseHTTPRequestHandler):
    protocol_version = 'HTTP/1.1'

    def log_message(self, *_):
        pass

    def do_POST(self):
        if self.path != '/v1/responses':
            self.send_error(404)
            return
        size = int(self.headers.get('Content-Length', '0'))
        if size > 8 * 1024 * 1024:
            self.send_error(413)
            return
        body = json.loads(self.rfile.read(size))
        strings = list(text_values(body.get('input', []))) + list(text_values(body.get('instructions', '')))
        loaded = {name: any(marker in value for value in strings) for name, marker in MARKERS.items()}
        # Persist only the controlled skill instructions and marker evidence, never headers or unrelated prompts.
        receipt = {'endpoint': self.path, 'authorizationPresent': self.headers.get('Authorization') is not None, 'model': body.get('model'), 'stream': body.get('stream'), 'loadedInstructionMarkers': loaded,
                   'skillInstructionBlocks': [value for value in strings if any(marker in value for marker in MARKERS.values())]}
        self.server.receipts.append(receipt)
        (self.server.artifacts / 'model-requests.json').write_text(json.dumps(self.server.receipts, indent=2) + '\n', encoding='utf-8')
        selected = [name for name, present in loaded.items() if present]
        text = MARKERS[selected[0]] if len(selected) == 1 else 'MPAI_SKILL_SELECTION_FAILED'
        response_id, message_id = 'resp_' + uuid.uuid4().hex, 'msg_' + uuid.uuid4().hex
        part = {'type': 'output_text', 'text': text, 'annotations': []}
        message = {'id': message_id, 'type': 'message', 'role': 'assistant', 'status': 'completed', 'content': [part]}
        self.send_response(200)
        self.send_header('Content-Type', 'text/event-stream')
        self.send_header('Connection', 'close')
        self.end_headers()

        def emit(kind, **fields):
            self.wfile.write(('event: ' + kind + '\ndata: ' + json.dumps({'type': kind, **fields}) + '\n\n').encode())
            self.wfile.flush()

        emit('response.created', response={'id': response_id, 'object': 'response', 'status': 'in_progress', 'output': []})
        emit('response.output_item.added', output_index=0, item={**message, 'status': 'in_progress', 'content': []})
        emit('response.content_part.added', item_id=message_id, output_index=0, content_index=0, part={**part, 'text': ''})
        emit('response.output_text.delta', item_id=message_id, output_index=0, content_index=0, delta=text)
        emit('response.output_text.done', item_id=message_id, output_index=0, content_index=0, text=text)
        emit('response.content_part.done', item_id=message_id, output_index=0, content_index=0, part=part)
        emit('response.output_item.done', output_index=0, item=message)
        emit('response.completed', response={'id': response_id, 'object': 'response', 'status': 'completed', 'output': [message], 'usage': {'input_tokens': 100, 'output_tokens': 10, 'total_tokens': 110}})
        self.close_connection = True


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=Path(__file__).resolve().parents[3])
    parser.add_argument('--artifacts', type=Path, required=True)
    parser.add_argument('--codex', type=Path, default=Path(shutil.which('codex') or '/missing/codex'))
    args = parser.parse_args()
    root, out = args.root.resolve(), args.artifacts.resolve()
    if out == root or root in out.parents:
        parser.error('Artifacts must be outside the repository')
    out.mkdir(parents=True, exist_ok=False)
    codex = args.codex.resolve(strict=True)
    binary = root / 'target/debug/mpai'
    env = {key: os.environ[key] for key in ['PATH', 'USER', 'LOGNAME', 'LANG'] if key in os.environ}
    home = out / 'home'
    config = home / '.codex'
    config.mkdir(parents=True)
    env.update(HOME=str(home), CODEX_HOME=str(config), XDG_CONFIG_HOME=str(home / '.config'), XDG_STATE_HOME=str(home / '.local/state'), XDG_DATA_HOME=str(home / '.local/share'), XDG_CACHE_HOME=str(home / '.cache'))
    version = subprocess.run([str(codex), '--version'], env=env, capture_output=True, text=True, check=True).stdout.strip()
    project = out / 'project'
    project.mkdir()
    skills = {}
    for name, marker in MARKERS.items():
        path = project / '.agents/skills' / name / 'SKILL.md'
        path.parent.mkdir(parents=True)
        path.write_text('---\nname: same-name\ndescription: Deterministic exact-path acceptance skill.\n---\n\nAlways reply exactly: ' + marker + '\n', encoding='utf-8')
        skills[name] = path
    server = ThreadingHTTPServer(('127.0.0.1', 0), Responses)
    server.daemon_threads = True
    server.artifacts, server.receipts = out, []
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    config.joinpath('config.toml').write_text('model = "gpt-5.3-codex"\nmodel_provider = "fixture"\napproval_policy = "never"\nsandbox_mode = "read-only"\n\n[model_providers.fixture]\nname = "Deterministic acceptance peer"\nbase_url = "http://127.0.0.1:' + str(server.server_port) + '/v1"\nwire_api = "responses"\nrequires_openai_auth = false\nsupports_websockets = false\nrequest_max_retries = 0\nstream_max_retries = 0\n', encoding='utf-8')
    spec = importlib.util.spec_from_file_location('live_client', root / 'tests/fixtures/live.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    client = module.Client(binary, out, env, 60)
    results = []
    try:
        client.call('runtime/initialize')
        for name, path in skills.items():
            session = client.call('session/create', {'harness': 'codex', 'cwd': str(project), 'model': 'gpt-5.3-codex', 'execution': {'configHome': str(config)}, 'config': {'executable': str(codex), 'modelProvider': 'fixture', 'approvalPolicy': 'never', 'sandbox': 'read-only'}})
            assert session['capabilities']['metadata']['nativeSkillInputs'] is True
            commands = client.call('agent/commands', {'sessionId': session['id']})
            same_name = [entry for entry in commands if entry['name'] == 'same-name']
            assert {entry['path'] for entry in same_name} == {str(item) for item in skills.values()}, same_name
            first_event = len(client.events)
            request_index = len(server.receipts)
            client.call('session/prompt', {'sessionId': session['id'], 'content': [{'type': 'text', 'data': 'Follow the explicitly selected native skill.'}, {'type': 'skill', 'data': {'name': 'same-name', 'path': str(path)}}]})
            deadline = time.monotonic() + 60
            while not any(event['type'] == 'turn.ended' for event in client.events[first_event:]):
                client.next(deadline)
            events = client.events[first_event:]
            terminal = next(event for event in events if event['type'] == 'turn.ended')
            assert terminal['data']['reason'] == 'completed', terminal
            text = ''.join(event['data']['text'] for event in events if event['type'] == 'text.delta')
            assert text == MARKERS[name], text
            requests = server.receipts[request_index:]
            assert requests, 'The actual native app-server did not call the local Responses endpoint'
            assert all(not request['authorizationPresent'] for request in requests)
            assert all(request['loadedInstructionMarkers'][name] for request in requests)
            assert all(not request['loadedInstructionMarkers'][other] for request in requests for other in MARKERS if other != name)
            results.append({'selectedName': 'same-name', 'selectedPath': str(path), 'nativeSkillsListed': same_name, 'runtimeSessionId': session['id'], 'nativeSessionId': session['native']['id'], 'modelRequests': len(requests), 'modelSawSelectedInstructionsOnly': True, 'reply': text, 'turnReason': terminal['data']['reason']})
            (out / 'results.json').write_text(json.dumps(results, indent=2) + '\n', encoding='utf-8')
            client.call('session/close', {'sessionId': session['id']})
        summary = {'installedNativeVersion': version, 'nativeCodexSha256': hashlib.sha256(codex.read_bytes()).hexdigest(), 'mpaiSha256': hashlib.sha256(binary.read_bytes()).hexdigest(), 'freshConfigHome': str(config), 'userCredentialsUsed': False, 'authorizationHeadersPresent': False, 'exactPathCasesPassed': len(results), 'sameNameDistinctPathsListed': True, 'actualResponsesRequests': len(server.receipts)}
        (out / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n', encoding='utf-8')
        print(json.dumps(summary))
    finally:
        client.close()
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)
        (out / 'cleanup.json').write_text(json.dumps({'runtimeExitCode': client.process.returncode, 'httpServerStopped': not thread.is_alive()}, indent=2) + '\n', encoding='utf-8')


if __name__ == '__main__':
    main()
