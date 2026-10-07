#!/usr/bin/env python3
"""Actual Pi CLI history and native GUI branching, prompt recovery and Git checkpoint QA."""
import argparse
import base64
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
HERE = Path(__file__).resolve()


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


native = load('native_desktop', HERE.parents[1] / 'desktop-gui/acceptance.py')
git_fixture = load('native_git_helpers', HERE.parents[1] / 'git-gui/acceptance.py')
FIRST, SECOND = 'HISTORY_FIRST_20261003', 'HISTORY_SECOND_20261003'
COMPLEX, THIRD, WRITE = 'COMPLEX_RECOVERY_20261003', 'LATER_MESSAGE_20261003', 'WRITE_CHECKPOINT_20261003'
BEFORE, AFTER = 'Before native checkpoint.\n', 'After actual native Pi write.\n'


class Model(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_POST(self):
        try:
            body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
            assert self.path == '/v1/chat/completions'
            users = [message for message in body['messages'] if message['role'] == 'user']
            content = users[-1]['content']
            parts = [{'type': 'text', 'text': content}] if isinstance(content, str) else content
            text = '\n'.join(part.get('text', '') for part in parts if part.get('type') == 'text')
            images = [part for part in parts if part.get('type') == 'image_url']
            receipt = {'model': body['model'], 'text': text, 'images': images,
                'userHistoryTexts': ['\n'.join(part.get('text', '') for part in message['content'])
                    if isinstance(message['content'], list) else message['content'] for message in users]}
            self.server.receipts.append(receipt)
            (self.server.out / 'model-requests.json').write_text(json.dumps(self.server.receipts, indent=2)+'\n')
            response = 'FIRST_NATIVE_RESPONSE' if FIRST in text else 'SECOND_NATIVE_RESPONSE' if SECOND in text else \
                'COMPLEX_NATIVE_RESPONSE' if COMPLEX in text else 'LATER_NATIVE_RESPONSE' if THIRD in text else 'NATIVE_ACK'
            last_user = max(index for index, message in enumerate(body['messages']) if message['role'] == 'user')
            tools = body['messages'][last_user+1:]
            if WRITE in text and not any(message['role'] == 'tool' for message in tools):
                assert any(tool['function']['name'] == 'write' for tool in body.get('tools', [])), 'Pi write tool unavailable'
                delta = {'role': 'assistant', 'tool_calls': [{'index': 0, 'id': 'call_'+uuid.uuid4().hex,
                    'type': 'function', 'function': {'name': 'write', 'arguments': json.dumps({'path': 'one.txt', 'content': AFTER})}}]}
                finish = 'tool_calls'
            else:
                delta, finish = {'role': 'assistant', 'content': 'CHECKPOINT_WRITTEN' if WRITE in text else response}, 'stop'
            self.send_response(200)
            self.send_header('Content-Type', 'text/event-stream')
            self.end_headers()
            chunk = {'id': 'history_'+uuid.uuid4().hex, 'object': 'chat.completion.chunk', 'created': int(time.time()), 'model': body['model']}
            for change, reason in [(delta, None), ({}, finish)]:
                self.wfile.write(('data: '+json.dumps(dict(chunk, choices=[{'index': 0, 'delta': change, 'finish_reason': reason}]))+'\n\n').encode())
            self.wfile.write(b'data: [DONE]\n\n')
            self.wfile.flush()
        except Exception as error:
            self.server.errors.append(str(error))
            self.close_connection = True


class HistoryDesktop(native.Desktop):
    def __init__(self, root, out, media):
        super().__init__(root, out)
        self.env = {key: value for key, value in self.env.items() if key in {
            'PATH', 'USER', 'LOGNAME', 'LANG', 'HOME', 'XDG_CONFIG_HOME', 'XDG_RUNTIME_DIR'}}
        self.env.update(NO_COLOR='1', GDK_BACKEND='x11')
        self.pi = root / 'tests/fixtures/harnesses/node_modules/.bin/pi'
        (out / 'bin/pi').symlink_to(self.pi)
        self.agent = out / 'pi-agent'
        self.agent.mkdir()
        self.env['PI_CODING_AGENT_DIR'] = str(self.agent)
        self.server = ThreadingHTTPServer(('127.0.0.1', 0), Model)
        self.server.daemon_threads = True
        self.server.receipts, self.server.errors, self.server.out = [], [], out
        threading.Thread(target=self.server.serve_forever, daemon=True).start()
        (self.agent / 'models.json').write_text(json.dumps({'providers': {'fixture': {
            'baseUrl': f'http://127.0.0.1:{self.server.server_port}/v1', 'api': 'openai-completions', 'apiKey': 'isolated-local-key',
            'models': [{'id': 'history', 'name': 'History fixture', 'reasoning': False, 'input': ['text', 'image'],
                'contextWindow': 65536, 'maxTokens': 1024, 'cost': {'input': 0, 'output': 0, 'cacheRead': 0, 'cacheWrite': 0}}]}}}))
        (self.agent / 'settings.json').write_text(json.dumps({'compaction': {'enabled': False}, 'retry': {'enabled': False}}))
        self.inputs = out / 'inputs'
        self.inputs.mkdir()
        for name in ['fixture.png', 'fixture.pdf']:
            shutil.copyfile(media / name, self.inputs / name)
        (self.inputs / 'fixture.txt').write_text('Exact attached text payload.\n')
        (self.project / 'one.txt').write_text(BEFORE)
        for args in [('init', '-b', 'main'), ('config', 'user.name', 'Fixture'), ('config', 'user.email', 'fixture@example.invalid'),
                ('add', '.'), ('commit', '-m', 'Initialize managed history fixture')]:
            self.git(*args, cwd=self.project)

    def git(self, *args, cwd=None):
        return subprocess.check_output(['git', '-C', str(cwd or self.tree), *args], env=self.env, stderr=subprocess.STDOUT, text=True).strip()

    def invoke(self, command, args=None, extension='git'):
        return self.rpc('extension/invoke', {'extensionId': extension, 'command': command, 'arguments': args or {}})

    def start(self):
        assets = self.artifacts / 'desktop-assets'
        shutil.copytree(self.root / 'apps/desktop/dist', assets)
        (self.artifacts / 'build.json').write_text(json.dumps({str(path): hashlib.sha256(path.read_bytes()).hexdigest()
            for path in [self.root / 'target/debug/mpai', self.root / 'apps/desktop/zig-out/bin/mpai-desktop', self.pi.resolve(), *sorted(assets.rglob('*.js'))]}, indent=2)+'\n')
        self.daemon = self.child([str(self.root / 'target/debug/mpai'), '--state-dir', str(self.artifacts / 'sessions'),
            'serve', '--http', '127.0.0.1:0', '--desktop-assets', str(assets), '--workbench',
            '--git-worktrees', str(self.artifacts / 'worktrees'), '--resources-root', str(self.artifacts / 'resources')], pipe=True)
        self.url = self.ready(self.daemon, 'HTTP listening on ').removesuffix('/rpc')
        opened = self.invoke('project.open', {'path': str(self.project)})
        self.project_id = opened['id']
        managed = self.invoke('worktree.create', {'projectId': self.project_id, 'branch': 'feature/native-history'})
        self.tree = Path(managed['path'])
        self.worktree_id = managed['id']
        # Real external CLI commands produce the source journal before GUI import.
        cli_dir = self.agent / 'sessions/external'
        cli_dir.mkdir(parents=True)
        initial = [str(self.pi), '--print', '--provider', 'fixture', '--model', 'history', '--no-builtin-tools',
            '--no-skills', '--no-prompt-templates', '--no-extensions', '--session-dir', str(cli_dir)]
        first = subprocess.run(initial+[FIRST], cwd=self.tree, env=self.env, capture_output=True, text=True, check=True, timeout=45)
        self.source = next(cli_dir.glob('*.jsonl'))
        second = subprocess.run(initial+['--session', str(self.source), SECOND], cwd=self.tree, env=self.env,
            capture_output=True, text=True, check=True, timeout=45)
        self.source_bytes = self.source.read_bytes()
        (self.artifacts / 'external-cli.json').write_text(json.dumps({'executable': str(self.pi), 'version': subprocess.check_output([str(self.pi), '--version'], env=self.env, text=True).strip(),
            'source': str(self.source), 'sourceSha256': hashlib.sha256(self.source_bytes).hexdigest(),
            'firstOutput': first.stdout, 'secondOutput': second.stdout, 'cwd': str(self.tree)}, indent=2)+'\n')
        self.session = self.rpc('session/create', {'harness': 'pi', 'cwd': str(self.tree), 'model': 'fixture/history',
            'config': {'executable': str(self.pi), 'args': ['--mode', 'rpc', '--provider', 'fixture']}})['id']
        self.rpc('session/metadata', {'sessionId': self.session, 'title': 'Import actual external Pi history'})
        xvfb = self.child(['Xvfb', '-displayfd', '1', '-screen', '0', '1600x1000x24', '-nolisten', 'tcp'], pipe=True)
        self.display = ':'+self.ready(xvfb, '')
        self.env.update(DISPLAY=self.display, MPAI_DESKTOP_URL=self.url+'/?session='+self.session,
            MPAI_DESKTOP_MANAGED='1', XDG_DATA_HOME=str(self.artifacts / 'xdg-data'), XDG_CACHE_HOME=str(self.artifacts / 'xdg-cache'))
        (self.artifacts / 'dbus.conf').write_text('<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN" "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd"><busconfig><type>session</type><listen>unix:tmpdir=/tmp</listen><policy context="default"><allow send_destination="*"/><allow receive_sender="*"/><allow own="*"/></policy></busconfig>')
        self.launch_host()
        return {'url': self.url, 'sessionId': self.session, 'display': self.display, 'window': self.window,
            'artifacts': str(self.artifacts), 'source': str(self.source), 'configHome': str(self.agent),
            'tree': str(self.tree), 'inputs': str(self.inputs)}

    def launch_host(self):
        previous = self.env['HOME']
        self.env['HOME'] = os.environ['HOME']
        try:
            super().launch_host()
        finally:
            self.env['HOME'] = previous

    def action(self, request):
        if request['action'] == 'type':
            self.x('windowfocus', '--sync', self.window)
            self.x('type', '--clearmodifiers', '--delay', '100', '--', request['text'])
            time.sleep(0.4)
            return {'action': 'type', 'completed': True}
        if request['action'] == 'wheel':
            self.x('mousemove', request['x'], request['y'])
            self.x('click', '--repeat', request.get('steps', 1), '--delay', '150', request.get('button', 5))
            time.sleep(0.4)
            return {'action': 'wheel', 'completed': True}
        if request['action'].startswith('dialog-'):
            windows = self.x('search', '--onlyvisible', '--name', '^Select Files$').splitlines()
            assert len(windows) == 1, 'Expected one owned file chooser on the private display'
            dialog = windows[0]
            self.x('windowfocus', '--sync', dialog)
            time.sleep(0.3)
            if request['action'] == 'dialog-key':
                self.x('key', '--clearmodifiers', request['key'])
            elif request['action'] == 'dialog-type':
                self.x('type', '--clearmodifiers', '--delay', '100', '--', request['text'])
            elif request['action'] == 'dialog-click':
                self.x('mousemove', '--window', dialog, request['x'], request['y'])
                time.sleep(0.3)
                self.x('click', 1)
            else:
                raise ValueError('Unsupported file chooser action')
            time.sleep(0.5)
            return {'action': request['action'], 'dialogWindow': dialog, 'completed': True}
        if request['action'] == 'capture-display':
            target = self.artifacts / (request['name']+'.png')
            subprocess.run(['ffmpeg', '-nostdin', '-loglevel', 'error', '-y', '-f', 'x11grab', '-video_size', '1600x1000',
                '-i', self.display, '-frames:v', '1', str(target)], stdin=subprocess.DEVNULL, check=True, env=self.env)
            return {'screenshot': str(target)}
        if request['action'] == 'verify-source':
            assert self.source.read_bytes() == self.source_bytes
            sessions = self.rpc('session/list')
            result = {'sourceUnchanged': True, 'sourceSha256': hashlib.sha256(self.source_bytes).hexdigest(),
                'sessions': [{'id': s['id'], 'native': s['native'], 'turnsCompleted': s['turnsCompleted'], 'state': s['state']} for s in sessions]}
            (self.artifacts / 'source-verified.json').write_text(json.dumps(result, indent=2)+'\n')
            return result
        if request['action'] == 'prepare-restore':
            assert (self.tree / 'one.txt').read_text() == AFTER
            self.git('add', 'one.txt')
            self.git('commit', '-m', 'Save actual native write before explicit restoration')
            sessions = self.rpc('session/list')
            keep = request.get('sessionId') or max(sessions,
                key=lambda session: Path(session['native']['id']).name)['id']
            for session in sessions:
                if session['id'] != keep and session['state'] not in ['closed', 'failed']:
                    self.rpc('session/close', {'sessionId': session['id']})
            self.restore_head = self.git('rev-parse', 'HEAD')
            return {'committedHead': self.restore_head, 'currentFile': AFTER, 'closedOtherOwnedSessions': True}
        if request['action'] == 'verify-restore':
            assert (self.tree / 'one.txt').read_text() == BEFORE
            assert self.git('rev-parse', 'HEAD') == self.restore_head
            assert (self.project / 'one.txt').read_text() == BEFORE
            result = {'managedFile': BEFORE, 'branchHeadUnchanged': True, 'mainCheckoutUnchanged': True,
                'head': self.restore_head, 'tree': str(self.tree)}
            (self.artifacts / 'restore-verified.json').write_text(json.dumps(result, indent=2)+'\n')
            return result
        return super().action(request)

    def close(self):
        owned = set().union(*(git_fixture.descendants(process.pid) for process in self.children))
        if hasattr(self, 'url'):
            try:
                for session in self.rpc('session/list'):
                    if session['state'] not in ['closed', 'failed']:
                        self.rpc('session/close', {'sessionId': session['id']})
            except Exception:
                pass
        super().close()
        self.server.shutdown()
        self.server.server_close()
        live = []
        for pid in sorted(owned):
            try:
                if Path(f'/proc/{pid}/stat').read_text().rsplit(')', 1)[1].split()[0] != 'Z':
                    live.append(pid)
            except OSError:
                pass
        (self.artifacts / 'descendant-cleanup.json').write_text(json.dumps({'ownedDescendantPids': sorted(owned),
            'remainingLive': live, 'localModelServerStopped': True, 'modelErrors': self.server.errors}, indent=2)+'\n')
        assert not live


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=HERE.parents[3])
    parser.add_argument('--artifacts', type=Path, required=True)
    parser.add_argument('--media', type=Path, required=True, help='External directory containing valid fixture.png and fixture.pdf')
    parser.add_argument('--script', type=Path, help='Run an initial JSON action array, then accept interactive JSON lines')
    args = parser.parse_args()
    root, out = args.root.resolve(), args.artifacts.resolve()
    if out.exists() or root == out or root in out.parents:
        parser.error('Use a fresh external artifact directory')
    desktop = HistoryDesktop(root, out, args.media.resolve())
    try:
        print(json.dumps({'ready': desktop.start()}), flush=True)
        initial = iter(json.loads(args.script.read_text())) if args.script else iter([])
        def requests():
            yield from initial
            for line in sys.stdin:
                if line.strip():
                    yield json.loads(line)
        for request in requests():
            request = json.loads(json.dumps(request).replace('{{artifacts}}', str(out)))
            try:
                result = {'request': request, 'result': desktop.action(request)}
            except Exception as error:
                result = {'request': request, 'error': str(error)}
            with (out / 'actions.jsonl').open('a') as ledger:
                ledger.write(json.dumps(result)+'\n')
            print(json.dumps(result), flush=True)
    finally:
        desktop.close()


if __name__ == '__main__':
    main()
