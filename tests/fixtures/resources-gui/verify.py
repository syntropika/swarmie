#!/usr/bin/env python3
"""Verify native resources acceptance receipts without revealing credential values."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import sqlite3


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--artifacts', type=Path, required=True)
    args = parser.parse_args()
    out = args.artifacts.resolve()
    actions = json.loads((out / 'actions.json').read_text())
    secret = (out / 'private/synthetic-secret').read_text()
    checks = []

    def check(name, condition, **evidence):
        if not condition:
            raise AssertionError(name)
        checks.append({'name': name, 'passed': True, **evidence})

    def results(method, command=None):
        return [a['result'] for a in actions if 'result' in a and a['request'].get('method') == method
                and (command is None or a['request'].get('params', {}).get('command') == command)]

    durable = json.loads((out / 'resources/resources.json').read_text())
    defaults = durable['projects'][str(out / 'project')]
    check('Project model, permission and workspace defaults are durable',
          defaults['harness'] == 'codex' and defaults['model'] == 'gpt-5.4'
          and defaults['config']['approvalPolicy'] == 'on-request'
          and defaults['config']['sandbox'] == 'read-only'
          and Path(defaults['workspace']).parent == out / 'worktrees')
    snapshots = results('session/list')[-1]
    by_model = {snapshot['options']['model']: snapshot for snapshot in snapshots}
    check('Actual sessions use project defaults and explicit restored conversation overrides',
          set(by_model) == {'gpt-5.4', 'gpt-5.2'}
          and by_model['gpt-5.2']['options']['cwd'] == str(out / 'project')
          and Path(by_model['gpt-5.4']['options']['cwd']).parent == out / 'worktrees'
          and all(snapshot['state'] == 'idle' and snapshot['turnsCompleted'] == 1 for snapshot in snapshots)
          and all(snapshot['options']['config']['approvalPolicy'] == 'on-request'
                  and snapshot['options']['config']['sandbox'] == 'read-only' for snapshot in snapshots))
    requests = json.loads((out / 'model-requests.json').read_text())
    expected_hash = hashlib.sha256(('Bearer ' + secret).encode()).hexdigest()
    check('Actual native CLI uses native authentication and GUI endpoint/model selections',
          len(requests) == 2 and {request['model'] for request in requests} == set(by_model)
          and all(request['authorizationPresent'] and request['authorizationSha256'] == expected_hash for request in requests)
          and 'http://127.0.0.1:1/v1' in (out / 'codex-config/config.toml').read_text(),
          models=[request['model'] for request in requests], authorizationRecorded='presence and SHA256 only')
    check('Native model input carries selected workspaces and interactive permission semantics',
          all(by_model[request['model']]['options']['cwd'] in json.dumps(request['input'])
              and 'read-only' in json.dumps(request['input'])
              and 'require_escalated' in json.dumps(request['input']) for request in requests))

    dashboards = results('extension/invoke', 'usage.dashboard')
    all_usage = next(report for report in reversed(dashboards) if report['totals']['observations'] == 2)
    check('Observed native usage ledger aggregates exact independent sessions',
          all_usage['totals']['inputTokens'] == 200 and all_usage['totals']['outputTokens'] == 20
          and all_usage['totals']['totalTokens'] == 220 and all_usage['totals']['unknownCosts'] == 2
          and set(all_usage['byModel']) == set(by_model)
          and all(value['observations'] == 1 and value['inputTokens'] == 100
                  and value['outputTokens'] == 10 for value in all_usage['byModel'].values()),
          totals=all_usage['totals'])
    gui_all = (out / 'usage-all-models.txt').read_text()
    gui_main = (out / 'usage-main-model.txt').read_text()
    gui_worktree = (out / 'usage-worktree-model.txt').read_text()
    check('Actual OS-rendered dashboard scope and all-project totals match backend',
          re.search(r'Total\s+200\s+[^\n]*20', gui_all) is not None and '2 reports' in gui_all
          and re.search(r'Total\s+100\s+10', gui_main) is not None and '1 reports' in gui_main
          and re.search(r'Total\s+100\s+10', gui_worktree) is not None and '1 reports' in gui_worktree
          and 'Unknown' in (out / 'usage-cost-columns.txt').read_text(),
          screenshots=['usage-all-models.png', 'usage-main-model.png', 'usage-worktree-model.png', 'usage-cost-columns.png'])
    check('Actual OS-rendered conversation recovers after native window restart',
          'gpt-5.2' in (out / 'defaults-after-restart.txt').read_text()
          and 'NATIVE_DEFAULTS_OK' in (out / 'defaults-native-response.txt').read_text())

    allowed = {out / 'private/synthetic-secret', out / 'resources/credentials.json'}
    leaks, native_files = [], []
    for path in out.rglob('*'):
        if path.is_file() and path not in allowed:
            data = path.read_bytes()
            if secret.encode() in data or secret.encode('utf-16le') in data:
                (native_files if out / 'codex-config' in path.parents else leaks).append(str(path.relative_to(out)))
    storage_found = False
    for path in (out / 'xdg-data').rglob('localstorage.sqlite3'):
        with sqlite3.connect(f'file:{path}?mode=ro', uri=True) as connection:
            for key, value in connection.execute('SELECT key,value FROM ItemTable'):
                if key == 'mpai.workspace.v2':
                    text = value.decode('utf-16le') if isinstance(value, bytes) else value
                    storage_found = 'gpt-5.2' in text
                    check('Decoded persistent workspace localStorage contains references, never credential values', secret not in text)
    check('Public receipts, snapshots, journals, and saved GUI state contain no plaintext fixture credential',
          not leaks and storage_found and (out / 'resources/credentials.json').stat().st_mode & 0o777 == 0o600
          and (out / 'codex-config').stat().st_mode & 0o777 == 0o700,
          nativePrivateCredentialCopies=native_files, nativeConfigHomeMode='0700', privateCredentialFileMode='0600')
    cleanup = json.loads((out / 'cleanup.json').read_text())
    check('All owned fixture processes terminate', cleanup['complete'])
    receipt = {'checks': checks, 'nativeGuiTested': True, 'upstreamAuthenticationTested': False,
               'knownCostVerified': False, 'nativeCredentialPersistence': 'Native CLI private configHome may retain environment snapshots'}
    (out / 'verification.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print(json.dumps({'passed': len(checks), 'receipt': str(out / 'verification.json')}))


if __name__ == '__main__':
    main()
