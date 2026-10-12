#!/usr/bin/env python3
"""Native GUI Git acceptance with installed Pi, a deterministic model and controlled hosting."""
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

fixture = Path(__file__).resolve()
spec = importlib.util.spec_from_file_location('native_desktop', fixture.parents[1] / 'desktop-gui/acceptance.py')
native = importlib.util.module_from_spec(spec)
spec.loader.exec_module(native)

MARKER = 'SWARMIE_REAL_DIFF_NATIVE_GUI_20261003'
CONFIG_MARKER = 'SWARMIE_RECOVERED_CONFIG_20261003'
COMMIT = 'Verify native Git workflow from real staged changes'
PR = {'title': 'Verify native Git workflow', 'body': 'The actual change adds ' + MARKER + '.\n\nValidation: controlled native GUI fixture.\n\nLiteral $(no-shell) and `text`.'}


def descendants(parent):
    parents = {}
    for path in Path('/proc').glob('[0-9]*/stat'):
        try:
            fields = path.read_text().rsplit(')', 1)[1].split()
            parents[int(path.parent.name)] = int(fields[1])
        except (OSError, ValueError, IndexError):
            continue
    found, frontier = set(), {parent}
    while frontier:
        frontier = {pid for pid, ppid in parents.items() if ppid in frontier and pid not in found}
        found.update(frontier)
    return found


class Model(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        users = [message for message in body['messages'] if message['role'] == 'user']
        content = users[-1]['content']
        prompt = content if isinstance(content, str) else '\n'.join(part.get('text', '') for part in content)
        try:
            assert self.path == '/v1/chat/completions'
            assert MARKER in prompt and '+'+MARKER in prompt, 'Generation did not contain actual changed diff'
            assert not body.get('tools'), 'Generation unexpectedly exposed native tools'
            systems = '\n'.join(message['content'] for message in body['messages'] if message['role'] == 'system')
            assert CONFIG_MARKER in systems, 'Recovered native configuration did not reach generation'
            kind = 'pr' if 'Return JSON with title and body' in prompt else 'commit'
            answer = json.dumps(PR) if kind == 'pr' else COMMIT
            self.server.receipts.append({'kind': kind, 'model': body['model'], 'prompt': prompt,
                'tools': body.get('tools', []), 'answer': answer, 'recoveredConfigMarker': CONFIG_MARKER in systems})
            (self.server.out / 'model-requests.json').write_text(json.dumps(self.server.receipts, indent=2)+'\n')
            self.send_response(200)
            self.send_header('Content-Type', 'text/event-stream')
            self.end_headers()
            base = {'id': 'git_gui_'+uuid.uuid4().hex, 'object': 'chat.completion.chunk',
                'created': int(time.time()), 'model': body['model']}
            for delta, finish in [({'role': 'assistant', 'content': answer}, None), ({}, 'stop')]:
                chunk = dict(base, choices=[{'index': 0, 'delta': delta, 'finish_reason': finish}])
                self.wfile.write(('data: '+json.dumps(chunk)+'\n\n').encode())
            self.wfile.write(b'data: [DONE]\n\n')
            self.wfile.flush()
        except Exception as error:
            self.server.errors.append(str(error))
            self.close_connection = True


class GitDesktop(native.Desktop):
    def __init__(self, root, out):
        super().__init__(root, out)
        self.env = {key: value for key, value in self.env.items() if key in {
            'PATH', 'USER', 'LOGNAME', 'LANG', 'HOME', 'XDG_CONFIG_HOME', 'XDG_RUNTIME_DIR'}}
        self.env.update(SWARMIE_GIT_GUI_STATE=str(out), NO_COLOR='1', GDK_BACKEND='x11')
        gh = out / 'bin/gh'
        gh.write_text('#!/bin/sh\nexec python3 '+str(fixture.with_name('gh_peer.py'))+' "$@"\n')
        gh.chmod(0o755)
        (out / 'bin/pi').symlink_to(root / 'tests/fixtures/harnesses/node_modules/.bin/pi')
        self.server = ThreadingHTTPServer(('127.0.0.1', 0), Model)
        self.server.daemon_threads = True
        self.server.receipts, self.server.errors, self.server.out = [], [], out
        threading.Thread(target=self.server.serve_forever, daemon=True).start()
        agent = out / 'pi-agent'
        agent.mkdir()
        self.env['PI_CODING_AGENT_DIR'] = str(agent)
        (agent / 'settings.json').write_text(json.dumps({'compaction': {'enabled': False}, 'retry': {'enabled': False}}))
        (agent / 'models.json').write_text(json.dumps({'providers': {'fixture': {
            'baseUrl': f'http://127.0.0.1:{self.server.server_port}/v1', 'api': 'openai-completions',
            'apiKey': 'isolated-local-key', 'models': [{'id': 'git', 'name': 'Git fixture', 'reasoning': False,
            'input': ['text'], 'contextWindow': 32768, 'maxTokens': 1024,
            'cost': {'input': 0, 'output': 0, 'cacheRead': 0, 'cacheWrite': 0}}]}}}))
        self.git('init', '-b', 'main')
        self.git('config', 'user.name', 'Fixture')
        self.git('config', 'user.email', 'fixture@example.invalid')
        self.git('add', '.')
        self.git('commit', '-m', 'Initialize isolated GUI fixture')
        self.remote = out / 'remote.git'
        subprocess.run(['git', 'init', '--bare', '-b', 'main', str(self.remote)], check=True, capture_output=True)
        self.git('remote', 'add', 'origin', str(self.remote))
        self.git('push', '--set-upstream', 'origin', 'main')
        self.git('checkout', '-b', 'feature/native-gui')
        (self.project / 'hello.txt').write_text('First fixture line.\n'+MARKER+'\nThird fixture line.\n')

    def git(self, *args, cwd=None):
        return subprocess.check_output(['git', '-C', str(cwd or self.project), *args], env=self.env, stderr=subprocess.STDOUT, text=True).strip()

    def start(self):
        assets = self.artifacts / 'desktop-assets'
        shutil.copytree(self.root / 'apps/desktop/dist', assets)
        pi = self.root / 'tests/fixtures/harnesses/node_modules/.bin/pi'
        self.config = {'executable': str(pi), 'args': ['--mode', 'rpc', '--provider', 'fixture',
            '--no-builtin-tools', '--no-skills', '--no-prompt-templates', '--append-system-prompt', CONFIG_MARKER]}
        (self.artifacts / 'build.json').write_text(json.dumps({str(path): hashlib.sha256(path.read_bytes()).hexdigest()
            for path in [self.root / 'target/debug/swarmie', self.root / 'apps/desktop/zig-out/bin/swarmie-desktop',
            pi.resolve(), *sorted(assets.rglob('*.js'))]}, indent=2)+'\n')
        daemon = self.child([str(self.root / 'target/debug/swarmie'), '--state-dir', str(self.artifacts / 'sessions'),
            'serve', '--http', '127.0.0.1:0', '--desktop-assets', str(assets), '--workbench',
            '--git-worktrees', str(self.artifacts / 'worktrees'), '--resources-root', str(self.artifacts / 'resources')], pipe=True)
        self.url = self.ready(daemon, 'HTTP listening on ').removesuffix('/rpc')
        self.session = self.rpc('session/create', {'harness': 'pi', 'cwd': str(self.project),
            'model': 'fixture/git', 'config': self.config})['id']
        self.rpc('session/metadata', {'sessionId': self.session, 'title': 'Native Git acceptance'})
        xvfb = self.child(['Xvfb', '-displayfd', '1', '-screen', '0', '1600x1000x24', '-nolisten', 'tcp'], pipe=True)
        self.display = ':'+self.ready(xvfb, '')
        self.env.update(DISPLAY=self.display, SWARMIE_DESKTOP_URL=self.url+'/?session='+self.session,
            SWARMIE_DESKTOP_MANAGED='1', XDG_DATA_HOME=str(self.artifacts / 'xdg-data'),
            XDG_CACHE_HOME=str(self.artifacts / 'xdg-cache'))
        (self.artifacts / 'dbus.conf').write_text('<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN" "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd"><busconfig><type>session</type><listen>unix:tmpdir=/tmp</listen><policy context="default"><allow send_destination="*"/><allow receive_sender="*"/><allow own="*"/></policy></busconfig>')
        self.launch_host()
        return {'url': self.url, 'sessionId': self.session, 'display': self.display,
            'window': self.window, 'artifacts': str(self.artifacts), 'project': str(self.project)}

    def launch_host(self):
        previous = self.env['HOME']
        self.env['HOME'] = os.environ['HOME']  # WebKit sandbox encloses the private XDG paths.
        try:
            super().launch_host()
        finally:
            self.env['HOME'] = previous

    def action(self, request):
        if request['action'] == 'type' and '\n' in request['text']:
            for index, line in enumerate(request['text'].split('\n')):
                if index:
                    self.x('key', '--clearmodifiers', 'Return')
                if line:
                    self.x('type', '--clearmodifiers', '--delay', '20', '--', line)
            return {'action': 'type', 'completed': True}
        if request['action'] == 'seed-created':
            target = self.artifacts / 'gui-created'
            assert (target / '.git').is_dir(), 'Create through GUI before seeding publication contents'
            (target / 'publication.txt').write_text('Controlled GUI publication fixture.\n')
            for args in [('config', 'user.name', 'Fixture'), ('config', 'user.email', 'fixture@example.invalid'),
                    ('add', 'publication.txt'), ('commit', '-m', 'Initialize created publication fixture')]:
                self.git(*args, cwd=target)
            result = {'reason': 'Seed committed content after GUI creation and before publication acceptance',
                'path': str(target), 'head': self.git('rev-parse', 'HEAD', cwd=target)}
            (self.artifacts / 'publication-setup.json').write_text(json.dumps(result, indent=2)+'\n')
            return result
        if request['action'] == 'verify-projects':
            state = json.loads((self.artifacts / 'hosting.json').read_text())
            repository = state['repositories'][0]
            created, cloned = self.artifacts / 'gui-created', self.artifacts / 'gui-cloned'
            heads = [self.git('rev-parse', 'HEAD', cwd=created),
                self.git('rev-parse', 'refs/heads/main', cwd=Path(repository['remote'])),
                self.git('rev-parse', 'HEAD', cwd=cloned)]
            assert len(set(heads)) == 1 and (created / 'publication.txt').read_bytes() == (cloned / 'publication.txt').read_bytes()
            scratch = list(self.artifacts.rglob('scratch-*'))
            assert any(path.is_dir() for path in scratch), 'Scratch project did not exist'
            result = {'heads': heads, 'sameClonedBytes': True, 'repository': repository,
                'scratchPaths': [str(path) for path in scratch if path.is_dir()]}
            (self.artifacts / 'projects-verified.json').write_text(json.dumps(result, indent=2)+'\n')
            return result
        if request['action'] == 'verify':
            state = json.loads((self.artifacts / 'hosting.json').read_text())
            head = self.git('rev-parse', 'HEAD')
            remote = self.git('rev-parse', 'refs/heads/feature/native-gui', cwd=self.remote)
            result = {'head': head, 'remoteHead': remote, 'commitMessage': self.git('log', '-1', '--format=%B'),
                'hosting': state, 'modelRequests': self.server.receipts, 'modelErrors': self.server.errors}
            assert head == remote and result['commitMessage'] == COMMIT and not self.server.errors
            assert {r['kind'] for r in self.server.receipts} == {'commit', 'pr'}
            assert state['pr']['isDraft'] and state['pr']['body'] == PR['body']
            assert state['comments'] and state['reviews']
            (self.artifacts / 'verified.json').write_text(json.dumps(result, indent=2)+'\n')
            return result
        if request['action'] == 'hosting-failure':
            (self.artifacts / 'hosting-failure.json').write_text(json.dumps({'operation': request['operation']}))
            return {'armed': request['operation']}
        return super().action(request)

    def close(self):
        owned = set().union(*(descendants(process.pid) for process in self.children))
        if hasattr(self, 'session'):
            try:
                self.rpc('session/close', {'sessionId': self.session})
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
        (self.artifacts / 'descendant-cleanup.json').write_text(json.dumps({
            'ownedDescendantPids': sorted(owned), 'remainingLive': live, 'localModelServerStopped': True}, indent=2)+'\n')
        assert not live, 'An owned native descendant remained after cleanup'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=fixture.parents[3])
    parser.add_argument('--artifacts', type=Path, required=True)
    parser.add_argument('--script', type=Path, help='JSON action array; otherwise interact with JSON lines on stdin')
    args = parser.parse_args()
    root, out = args.root.resolve(), args.artifacts.resolve()
    if root == out or root in out.parents or out.exists():
        parser.error('Use a fresh external artifact directory')
    desktop = GitDesktop(root, out)
    try:
        print(json.dumps({'ready': desktop.start()}), flush=True)
        requests = json.loads(args.script.read_text()) if args.script else (json.loads(line) for line in sys.stdin if line.strip())
        failures = 0
        for request in requests:
            if isinstance(request.get('text'), str):
                request = {**request, 'text': request['text'].replace('{{artifacts}}', str(out))}
            try:
                result = {'request': request, 'result': desktop.action(request)}
            except Exception as error:
                result = {'request': request, 'error': str(error)}
                failures += 1
            with (out / 'actions.jsonl').open('a') as ledger:
                ledger.write(json.dumps(result)+'\n')
            print(json.dumps(result), flush=True)
        if args.script and failures:
            raise SystemExit(1)
    finally:
        desktop.close()


if __name__ == '__main__':
    main()
