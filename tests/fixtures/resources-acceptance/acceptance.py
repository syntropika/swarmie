#!/usr/bin/env python3
"""Run real Pi workflow acceptance against an isolated daemon and local model peer."""
import argparse
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import threading
import time
import urllib.error
import urllib.request
import uuid


def content_text(message):
    content = message.get('content', '')
    return content if isinstance(content, str) else '\n'.join(part.get('text', '') for part in content)


def process_children(parent):
    """Find only descendants of our daemon, without recording command lines or environment."""
    parents = {}
    for path in Path('/proc').glob('[0-9]*/stat'):
        try:
            fields = path.read_text().rsplit(')', 1)[1].split()
            parents[int(path.parent.name)] = int(fields[1])
        except (OSError, ValueError, IndexError):
            continue
    found = set()
    frontier = {parent}
    while frontier:
        frontier = {pid for pid, ppid in parents.items() if ppid in frontier and pid not in found}
        found.update(frontier)
    return found


class Peer(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_POST(self):
        try:
            body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
            assert self.path == '/v1/chat/completions', self.path
            messages = body['messages']
            user_index = max(i for i, message in enumerate(messages) if message['role'] == 'user')
            user = content_text(messages[user_index])
            tool_results = [message for message in messages[user_index + 1:] if message['role'] == 'tool']
            receipt = {'model': body['model'], 'prompt': user, 'toolResults': tool_results,
                       'tools': [tool['function']['name'] for tool in body.get('tools', [])]}
            self.server.receipts.append(receipt)
            tool = None
            if user == 'ASK_PERMISSION' and not tool_results:
                tool = ('fixture_confirm', {})
            elif user == 'ORCHESTRATE' and len(tool_results) < 3:
                assert 'peers_spawn' in receipt['tools'], receipt
                if not tool_results:
                    tool = ('peers_spawn', {'harness': 'pi', 'model': 'fixture/alpha', 'cwd': str(self.server.workspace)})
                else:
                    spawned = json.loads(content_text(tool_results[0]))
                    session = spawned['session']['id']
                    tool = (('peers_send', {'sessionId': session, 'text': 'ORCH_PEER'})
                            if len(tool_results) == 1 else ('peers_read', {'sessionId': session, 'limit': 4096}))
            if tool:
                name, arguments = tool
                delta = {'role': 'assistant', 'tool_calls': [{'index': 0, 'id': 'call_' + uuid.uuid4().hex,
                         'type': 'function', 'function': {'name': name, 'arguments': json.dumps(arguments)}}]}
                finish = 'tool_calls'
            else:
                if user == 'ASK_PERMISSION':
                    assert 'NATIVE_CONFIRM_YES' in content_text(tool_results[-1]), tool_results
                if user == 'ORCHESTRATE':
                    assert json.loads(content_text(tool_results[1]))['delivery'] == 'started'
                delta = {'role': 'assistant', 'content': body['model'].upper() + ':' + user + ':OK'}
                finish = 'stop'
            self.send_response(200)
            self.send_header('Content-Type', 'text/event-stream')
            self.end_headers()
            base = {'id': 'fixture_' + uuid.uuid4().hex, 'object': 'chat.completion.chunk',
                    'created': int(time.time()), 'model': body['model']}
            for change, reason in [(delta, None), ({}, finish)]:
                item = dict(base, choices=[{'index': 0, 'delta': change, 'finish_reason': reason}])
                self.wfile.write(('data: ' + json.dumps(item) + '\n\n').encode())
                self.wfile.flush()
            usage = dict(base, choices=[], usage={'prompt_tokens': 100, 'completion_tokens': 10, 'total_tokens': 110})
            self.wfile.write(('data: ' + json.dumps(usage) + '\n\ndata: [DONE]\n\n').encode())
            self.wfile.flush()
        except Exception as error:
            self.server.errors.append(repr(error))
            self.close_connection = True


class Daemon:
    def __init__(self, root, out, env):
        self.root, self.out, self.env = root, out, env
        self.serial = 0
        self.receipts = []
        self.process = None

    def start(self):
        with socket.socket() as sock:
            sock.bind(('127.0.0.1', 0))
            port = sock.getsockname()[1]
        self.url = f'http://127.0.0.1:{port}/rpc'
        self.env.update(SWARMIE_RPC_URL=self.url, SWARMIE_TOKEN="isolated-fixture-token")
        self.log = (self.out / 'daemon.log').open('a')
        self.process = subprocess.Popen([str(self.root / 'target/debug/swarmie'), '--state-dir', str(self.out / 'sessions'),
            'serve', '--http', f'127.0.0.1:{port}', '--token', 'isolated-fixture-token',
            '--resources-root', str(self.out / 'resources'), '--git-worktrees', str(self.out / 'worktrees')],
            env=self.env, stdout=self.log, stderr=self.log)
        wait(lambda: self.call('runtime/initialize'), 'daemon readiness', retry_errors=True)

    def stop(self):
        if self.process and self.process.poll() is None:
            children = process_children(self.process.pid)
            self.process.send_signal(signal.SIGINT)
            try:
                self.process.wait(timeout=15)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait()
                raise AssertionError('daemon failed graceful shutdown')
            def stopped():
                alive = []
                for pid in children:
                    try:
                        fields = Path(f'/proc/{pid}/stat').read_text().rsplit(')', 1)[1].split()
                        if fields[0] != 'Z':
                            alive.append(pid)
                    except OSError:
                        pass
                return not alive
            wait(stopped, 'owned daemon children cleanup')
        if hasattr(self, 'log'):
            self.log.close()

    def call(self, method, params=None):
        self.serial += 1
        req = urllib.request.Request(self.url, json.dumps({'jsonrpc': '2.0', 'id': self.serial,
            'method': method, 'params': params or {}}).encode(), headers={
                'Content-Type': 'application/json', 'Authorization': 'Bearer isolated-fixture-token'})
        with urllib.request.urlopen(req, timeout=40) as response:
            value = json.load(response)
        self.receipts.append({'method': method, 'params': params or {}, 'response': value})
        if 'error' in value:
            raise RuntimeError(json.dumps(value['error']))
        return value['result']

    def extension(self, command, arguments=None, extension='resources'):
        return self.call('extension/invoke', {'extensionId': extension, 'command': command, 'arguments': arguments or {}})

    def events(self, session):
        return self.extension('peers.read', {'sessionId': session, 'limit': 4096})['events']

    def settled(self, session, turns=1):
        def done():
            snapshot = self.call('session/get', {'sessionId': session})
            events = self.events(session)
            if snapshot['state'] == 'failed':
                raise AssertionError(events)
            return events if snapshot['state'] == 'idle' and sum(e['type'] == 'turn.ended' for e in events) >= turns else None
        return wait(done, 'native session settlement')


def wait(check, label, retry_errors=False):
    end = time.monotonic() + 60
    while time.monotonic() < end:
        try:
            result = check()
            if result:
                return result
        except (OSError, RuntimeError):
            if not retry_errors:
                raise
        time.sleep(.08)
    raise TimeoutError(label)


def text(events):
    return ''.join(event['data']['text'] for event in events if event['type'] == 'text.delta')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=Path(__file__).resolve().parents[3])
    parser.add_argument('--artifacts', type=Path, required=True)
    args = parser.parse_args()
    root, out = args.root.resolve(), args.artifacts.resolve()
    if out == root or root in out.parents:
        parser.error('Artifacts must be outside the repository')
    out.mkdir(parents=True, exist_ok=False)
    home, workspace, agent = out / 'home', out / 'project', out / 'pi-agent'
    for path in [home, workspace, agent]:
        path.mkdir()
    env = {key: os.environ[key] for key in ['PATH', 'USER', 'LOGNAME', 'LANG'] if key in os.environ}
    env.update(HOME=str(home), PI_CODING_AGENT_DIR=str(agent), NO_COLOR='1',
               XDG_CONFIG_HOME=str(home / '.config'), XDG_CACHE_HOME=str(home / '.cache'),
               XDG_DATA_HOME=str(home / '.local/share'), XDG_STATE_HOME=str(home / '.local/state'))
    pi = root / 'tests/fixtures/harnesses/node_modules/.bin/pi'
    server = ThreadingHTTPServer(('127.0.0.1', 0), Peer)
    server.daemon_threads = True
    server.receipts, server.errors, server.workspace = [], [], workspace
    threading.Thread(target=server.serve_forever, daemon=True).start()
    models = [{'id': name, 'name': name, 'reasoning': False, 'input': ['text'], 'contextWindow': 32768,
               'maxTokens': 1024, 'cost': {'input': rate, 'output': rate * 2, 'cacheRead': 0, 'cacheWrite': 0}}
              for name, rate in [('alpha', 2), ('beta', 3), ('orchestrator', 1)]]
    (agent / 'models.json').write_text(json.dumps({'providers': {'fixture': {
        'baseUrl': f'http://127.0.0.1:{server.server_port}/v1', 'api': 'openai-completions',
        'apiKey': 'isolated-local-key', 'models': models}}}))
    (agent / 'settings.json').write_text(json.dumps({'compaction': {'enabled': False}, 'retry': {'enabled': False}}))
    subprocess.run(['git', 'init', '-b', 'main', str(workspace)], check=True, capture_output=True)
    (workspace / 'hello.txt').write_text('Controlled workflow fixture.\n')
    for command in [['git', '-C', str(workspace), 'add', 'hello.txt'],
                    ['git', '-C', str(workspace), '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid', 'commit', '-m', 'Initialize fixture']]:
        subprocess.run(command, check=True, capture_output=True)
    daemon = Daemon(root, out, env)
    report = {'nativeVersion': subprocess.check_output([str(pi), '--version'], env=env, text=True).strip(),
              'nativeSha256': hashlib.sha256(pi.resolve().read_bytes()).hexdigest(),
              'daemonSha256': hashlib.sha256((root / 'target/debug/swarmie').read_bytes()).hexdigest(),
              'productionAuthTested': False, 'checks': {}, 'modelPricing': models}
    sessions = set()
    try:
        daemon.start()
        native_args = ['--mode', 'rpc', '--provider', 'fixture', '--no-builtin-tools', '--no-skills', '--no-prompt-templates',
                       '--extension', str(root / 'extensions/resources/pi/peer-tools.ts'),
                       '--extension', str(Path(__file__).with_name('confirm.ts').resolve())]
        native_config = {'executable': str(pi), 'args': native_args}
        daemon.extension('project.set', {'cwd': str(workspace), 'harness': 'pi', 'model': 'fixture/alpha', 'config': native_config})
        scheduled = daemon.extension('session.create', {'cwd': str(workspace), 'harness': 'pi', 'model': 'fixture/alpha'})
        sessions.add(scheduled['id'])
        daemon.extension('schedules.put', {'id': 'once', 'sessionId': scheduled['id'], 'text': 'SCHEDULE_ONCE', 'nextRunAtMs': int(time.time() * 1000) + 400})
        events = daemon.settled(scheduled['id'])
        assert text(events) == 'ALPHA:SCHEDULE_ONCE:OK', events
        time.sleep(.65)
        schedule = daemon.extension('schedules.list')['schedules'][0]
        assert schedule['status'] == 'delivered' and schedule['enabled'] is False, schedule
        assert sum(e['type'] == 'turn.ended' for e in daemon.events(scheduled['id'])) == 1
        report['checks']['oneShotClockNativePrompt'] = True
        peer = daemon.extension('peers.spawn', {'cwd': str(workspace), 'harness': 'pi', 'model': 'fixture/alpha', 'text': 'ASK_PERMISSION'})
        peer_id = peer['session']['id']
        sessions.add(peer_id)
        pending = wait(lambda: [e for e in daemon.events(peer_id) if e['type'] == 'permission.requested'], 'native confirm dialog')[-1]
        assert daemon.call('session/get', {'sessionId': peer_id})['state'] == 'needs_input'
        delivery = daemon.extension('peers.send', {'sessionId': peer_id, 'text': 'QUEUED_AFTER_CONFIRM'})
        assert delivery['delivery'] == 'queued' and delivery['queueId'], delivery
        time.sleep(.3)
        assert not any(e['type'] == 'turn.ended' for e in daemon.events(peer_id))
        daemon.call('permission/respond', {'sessionId': peer_id, 'id': pending['data']['id'], 'option': 'answer', 'answers': {'value': 'yes'}})
        events = daemon.settled(peer_id, 2)
        assert text(events) == 'ALPHA:ASK_PERMISSION:OKALPHA:QUEUED_AFTER_CONFIRM:OK', events
        assert any(e['type'] == 'permission.resolved' for e in events)
        read = daemon.extension('peers.read', {'sessionId': peer_id, 'afterSequence': pending['sequence'], 'limit': 4096})
        assert read['durable'] is True and all(e['sequence'] > pending['sequence'] for e in read['events'])
        report['checks']['nativePeerPermissionQueueReadSend'] = True
        orchestrator = daemon.extension('peers.spawn', {'cwd': str(workspace), 'harness': 'pi', 'model': 'fixture/orchestrator', 'text': 'ORCHESTRATE'})
        sessions.add(orchestrator['session']['id'])
        events = daemon.settled(orchestrator['session']['id'])
        completed = [e['data']['name'] for e in events if e['type'] == 'tool.completed']
        assert completed == ['peers_spawn', 'peers_send', 'peers_read'], completed
        native_peers = daemon.extension('peers.list')['sessions']
        spawned = next(s for s in native_peers if s['id'] not in sessions)
        sessions.add(spawned['id'])
        assert spawned['options']['harness'] == 'pi' and spawned['options']['model'] == 'fixture/alpha'
        assert text(daemon.settled(spawned['id'])) == 'ALPHA:ORCH_PEER:OK'
        report['checks']['modelNativePeerBridgeThreeOperations'] = True
        project = daemon.extension('project.open', {'path': str(workspace)}, 'git')
        trees = [daemon.extension('worktree.create', {'projectId': project['id'], 'branch': 'fixture-' + model}, 'git') for model in ['alpha', 'beta']]
        for tree in trees:
            daemon.extension('project.set', {'cwd': tree['path'], 'harness': 'pi', 'config': native_config})
        job = daemon.extension('fanout.start', {'cwd': str(workspace), 'text': 'SAME_TASK_COMPARISON', 'targets': [
            {'harness': 'pi', 'model': 'fixture/' + model, 'cwd': tree['path']} for model, tree in zip(['alpha', 'beta'], trees)]})
        def finished():
            value = daemon.extension('fanout.status', {'id': job['id']})
            return value if value['status'] != 'running' else None
        comparison = wait(finished, 'native independent comparison')
        assert comparison['status'] == 'completed', comparison
        ids = [target['sessionId'] for target in comparison['targets']]
        assert len(set(ids)) == 2
        sessions.update(ids)
        for target, model, tree in zip(comparison['targets'], ['alpha', 'beta'], trees):
            assert target['text'] == model.upper() + ':SAME_TASK_COMPARISON:OK', target
            snapshot = daemon.call('session/get', {'sessionId': target['sessionId']})
            assert snapshot['options']['cwd'] == tree['path'] and snapshot['options']['model'] == 'fixture/' + model
        assert len({daemon.call('session/get', {'sessionId': sid})['native']['id'] for sid in ids}) == 2
        report['checks']['twoModelIndependentManagedWorktrees'] = True
        beta_dashboard = daemon.extension('usage.dashboard', {'cwd': trees[1]['path'], 'harness': 'pi'})
        assert set(beta_dashboard['sessions']) == {ids[1]}, beta_dashboard
        assert set(beta_dashboard['byModel']) == {'fixture/beta'}
        assert beta_dashboard['totals']['inputTokens'] == 100 and beta_dashboard['totals']['outputTokens'] == 10
        assert abs(beta_dashboard['totals']['knownCostUsd'] - .00036) < 1e-10
        for tree, sid in zip(trees, ids):
            try:
                daemon.extension('worktree.remove', {'projectId': project['id'], 'worktreeId': tree['id']}, 'git')
                raise AssertionError('active-session worktree removal was allowed')
            except RuntimeError as error:
                assert 'busy' in str(error).lower(), error
            daemon.call('session/close', {'sessionId': sid})
            assert daemon.extension('worktree.remove', {'projectId': project['id'], 'worktreeId': tree['id']}, 'git')['removed'] is True
            assert not Path(tree['path']).exists()
        report['checks']['safeManagedWorktreeCleanup'] = True
        dashboard = daemon.extension('usage.dashboard')
        native_usage = [e['data'] for sid in sessions for e in daemon.events(sid) if e['type'] == 'usage.updated']
        assert len(native_usage) > 6 and dashboard['totals']['observations'] == len(native_usage), (dashboard, native_usage)
        for key in ['inputTokens', 'outputTokens', 'totalTokens']:
            assert dashboard['totals'][key] == sum(usage[key] for usage in native_usage)
        assert abs(dashboard['totals']['knownCostUsd'] - sum(usage['costUsd'] for usage in native_usage)) < 1e-10
        assert dashboard['totals']['knownCostUsd'] > 0 and dashboard['unknownCosts'] == 0
        assert set(dashboard['byModel']) == {'fixture/alpha', 'fixture/beta', 'fixture/orchestrator'}
        report['dashboard'] = dashboard
        report['filteredDashboard'] = beta_dashboard
        for sid in sessions:
            if daemon.call('session/get', {'sessionId': sid})['state'] != 'closed':
                daemon.call('session/close', {'sessionId': sid})
        scheduled_events = daemon.events(scheduled['id'])
        daemon.stop()
        daemon.start()
        retained = daemon.extension('usage.dashboard')
        assert retained == dashboard, (retained, dashboard)
        listed = daemon.extension('fanout.list')['jobs']
        assert any(item['id'] == job['id'] and item['status'] == 'completed' for item in listed)
        assert daemon.extension('fanout.status', {'id': job['id']}) == comparison
        assert daemon.extension('fanout.remove', {'id': job['id']})['removed'] is True
        assert not daemon.extension('fanout.list')['jobs']
        schedule = daemon.extension('schedules.list')['schedules'][0]
        assert schedule['enabled'] is False and schedule['status'] == 'delivered'
        assert daemon.events(scheduled['id']) == scheduled_events
        assert not server.errors, server.errors
        report['checks']['durableObservedUsageCostsAndComparison'] = True
        report['checks']['nativeProcessesGracefullyStopped'] = True
    except Exception as error:
        report['failure'] = repr(error)
        raise
    finally:
        daemon.stop()
        server.shutdown()
        server.server_close()
        report['modelRequests'] = len(server.receipts)
        report['modelErrors'] = server.errors
        for name, value in [('result.json', report), ('rpc-receipts.json', daemon.receipts), ('model-requests.json', server.receipts)]:
            (out / name).write_text(json.dumps(value, indent=2) + '\n')
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
