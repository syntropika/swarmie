#!/usr/bin/env python3
"""Stateful isolated hosting CLI peer; real local Git transport, no network or merge."""
import fcntl
import json
import os
from pathlib import Path
import stat
import subprocess
import sys

root = Path(os.environ['SWARMIE_GIT_GUI_STATE']).resolve()
args = sys.argv[1:]
with (root / 'hosting.lock').open('a') as lock:
    fcntl.flock(lock, fcntl.LOCK_EX)
    state_path = root / 'hosting.json'
    state = json.loads(state_path.read_text()) if state_path.exists() else {
        'pr': None, 'comments': [], 'reviews': [], 'viewed': {'hello.txt': False}, 'repositories': []}
    receipt = {'args': args, 'cwd': os.getcwd()}
    if '--body-file' in args:
        body_path = Path(args[args.index('--body-file') + 1])
        receipt.update(body=body_path.read_text(), bodyMode=oct(stat.S_IMODE(body_path.stat().st_mode)))
        assert receipt['bodyMode'] == '0o600'
    operation = ' '.join(args[:2])
    failure = root / 'hosting-failure.json'
    if failure.exists() and json.loads(failure.read_text())['operation'] == operation:
        failure.unlink()
        receipt['error'] = 'Controlled hosting rejection'
        with (root / 'hosting.jsonl').open('a') as ledger:
            ledger.write(json.dumps(receipt) + '\n')
        sys.exit('Controlled hosting rejection')
    output, code = '', 0
    if args == ['--version']:
        output = 'gh version controlled-gui-peer'
    elif operation == 'auth status':
        output = 'Isolated hosting account is authenticated.'
    elif operation == 'pr create':
        assert '--draft' in args and state['pr'] is None
        state['pr'] = {'number': 39, 'url': 'https://hosting.example.invalid/fixture/project/pull/39',
            'title': args[args.index('--title') + 1], 'body': receipt['body'], 'state': 'OPEN',
            'isDraft': True, 'reviewDecision': 'REVIEW_REQUIRED', 'headRefName': 'feature/native-gui',
            'baseRefName': args[args.index('--base') + 1], 'files': [{'path': 'hello.txt'}]}
        output = state['pr']['url']
    elif operation == 'pr view':
        if not state['pr']:
            sys.exit('No controlled pull request exists yet')
        fields = args[args.index('--json') + 1]
        output = json.dumps({'id': 'PR_GUI'} if fields == 'id' else
            {'comments': state['comments'], 'reviews': state['reviews']} if fields == 'comments,reviews'
            else state['pr'])
    elif operation == 'pr checks':
        output = json.dumps([{'name': 'controlled-ci', 'state': 'FAILURE', 'bucket': 'fail',
            'link': 'https://hosting.example.invalid/check/1', 'workflow': 'Fixture CI'}])
        code = 1  # Real gh uses nonzero for failed checks; structured results must still render.
    elif operation == 'pr comment':
        state['comments'].append({'body': receipt['body'], 'author': {'login': 'fixture-reviewer'},
            'url': state['pr']['url'] + '#comment-' + str(len(state['comments']) + 1)})
        output = state['comments'][-1]['url']
    elif operation == 'pr review':
        decision = 'APPROVED' if '--approve' in args else 'CHANGES_REQUESTED' if '--request-changes' in args else 'COMMENTED'
        state['reviews'].append({'body': receipt['body'], 'state': decision, 'author': {'login': 'fixture-reviewer'}})
        state['pr']['reviewDecision'] = decision
    elif operation == 'api graphql':
        query = next(value[6:] for value in args if value.startswith('query='))
        if 'viewerViewedState' in query:
            output = json.dumps({'data': {'node': {'files': {'nodes': [
                {'path': path, 'viewerViewedState': 'VIEWED' if viewed else 'UNVIEWED'}
                for path, viewed in state['viewed'].items()], 'pageInfo': {'hasNextPage': False}}}}})
        else:
            path = next(value[5:] for value in args if value.startswith('path='))
            assert path in state['viewed']
            action = 'unmarkFileAsViewed' if 'unmarkFileAsViewed' in query else 'markFileAsViewed'
            state['viewed'][path] = action == 'markFileAsViewed'
            output = json.dumps({'data': {action: {'clientMutationId': None}}})
    elif operation == 'repo create':
        source = Path(args[args.index('--source') + 1]).resolve()
        assert root in source.parents and '--push' in args and '--private' in args
        name = args[2]
        remote = root / ('published-' + name + '.git')
        assert not remote.exists()
        subprocess.run(['git', 'init', '--bare', '-b', 'main', str(remote)], check=True, capture_output=True)
        subprocess.run(['git', '-C', str(source), 'remote', 'add', 'origin', str(remote)], check=True, capture_output=True)
        branch = subprocess.check_output(['git', '-C', str(source), 'branch', '--show-current'], text=True).strip()
        subprocess.run(['git', '-C', str(source), 'push', '--set-upstream', 'origin', branch], check=True, capture_output=True)
        output = 'https://hosting.example.invalid/fixture/' + name
        state['repositories'].append({'name': name, 'source': str(source), 'remote': str(remote), 'branch': branch, 'url': output})
    else:
        raise SystemExit('Unsupported controlled operation; merge is never allowed')
    receipt.update(result=output, exitCode=code)
    state_path.write_text(json.dumps(state, indent=2) + '\n')
    with (root / 'hosting.jsonl').open('a') as ledger:
        ledger.write(json.dumps(receipt) + '\n')
    print(output)
    sys.exit(code)
