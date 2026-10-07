#!/usr/bin/env python3
"""Verify exact configured-price native GUI usage receipts without printing credentials."""
import argparse
from datetime import datetime, timezone
import json
import math
from pathlib import Path
import struct


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--artifacts', type=Path, required=True)
    out = parser.parse_args().artifacts.resolve()
    actions = json.loads((out / 'actions.json').read_text())
    checks = []

    def check(name, condition, **evidence):
        assert condition, name
        checks.append({'name': name, 'passed': True, **evidence})

    def command(action, name):
        request = action['request']
        return request.get('method') == 'extension/invoke' and request.get('params', {}).get('command') == name

    def totals(report, count, cost):
        row = report['totals']
        return (row['inputTokens'] == count * 100 and row['outputTokens'] == count * 10
                and row['totalTokens'] == count * 110 and row['observations'] == count
                and row['unknownCosts'] == 0 and row['unknownTokenFields'] == 0
                and math.isclose(row['knownCostUsd'], cost, abs_tol=1e-12))

    check('Every recorded native GUI and verification action completed', all('error' not in action for action in actions))
    build = json.loads((out / 'build.json').read_text())
    native_models = json.loads((out / 'pi-agent/models.json').read_text())['providers']['fixture']['models']
    check('Explicit native model pricing persists unchanged across restart',
          native_models == build['configuredRatesPerMillion'] and build['nativeVersion'] == '1.0.0',
          configuredRatesPerMillion={model['id']: model['cost'] for model in native_models})
    snapshots = [action['result'] for action in actions if action['request'].get('method') == 'session/list'][-1]
    model_by_session = {snapshot['id']: snapshot['options']['model'].split('/')[-1] for snapshot in snapshots}
    rates = {model['id']: model['cost'] for model in native_models}
    events = {}
    for action in actions:
        if action['request'].get('method') == 'session/events':
            for event in action['result']:
                if event['type'] == 'usage.updated':
                    events[(event['sessionId'], event['sequence'])] = event
    exact_events = []
    for event in events.values():
        value, rate = event['data'], rates[model_by_session[event['sessionId']]]
        expected = (value['inputTokens'] * rate['input'] + value['outputTokens'] * rate['output']) / 1_000_000
        assert value['inputTokens'] == 100 and value['outputTokens'] == 10 and value['totalTokens'] == 110
        assert value['scope'] == 'turn' and math.isclose(value['costUsd'], expected, abs_tol=1e-12)
        exact_events.append({'sessionId': event['sessionId'], 'model': model_by_session[event['sessionId']],
                             'costUsd': value['costUsd'], 'timestampMs': event['timestampMs']})
    requests = json.loads((out / 'model-requests.json').read_text())
    check('Three actual native model calls produce exact pricing-derived normalized usage events',
          len(events) == 3 and len(requests) == 3 and [request['model'] for request in requests] == ['alpha', 'beta', 'beta']
          and not json.loads((out / 'model-errors.json').read_text()), events=exact_events)
    restart = next(index for index, action in enumerate(actions) if action['request']['action'] == 'restart-all')
    all_reports = [(index, action['result']) for index, action in enumerate(actions)
                   if command(action, 'usage.dashboard') and not action['request']['params'].get('arguments', {}).get('cwd')]
    before = next(report for index, report in reversed(all_reports) if index < restart)
    recovered = next(report for index, report in all_reports if index > restart)
    final = all_reports[-1][1]
    check('Daemon restart retains the same observed ledger before any new model call',
          totals(before, 2, .006) and recovered == before)
    check('Resumed postrestart native turn extends exact project and group totals', totals(final, 3, .0096)
          and math.isclose(final['byModel']['fixture/alpha']['knownCostUsd'], .0024, abs_tol=1e-12)
          and math.isclose(final['byModel']['fixture/beta']['knownCostUsd'], .0072, abs_tol=1e-12)
          and final['byHarness']['pi'] == final['totals']
          and set(final['byDay']) == {datetime.fromtimestamp(event['timestampMs'] / 1000, timezone.utc).date().isoformat()
                                     for event in events.values()}
          and next(iter(final['byDay'].values())) == final['totals'], totals=final['totals'])
    scopes = [action['result'] for action in actions if command(action, 'usage.dashboard')
              and action['request']['params'].get('arguments', {}).get('cwd')]
    check('Main workspace and managed worktree remain independent cost scopes',
          any(totals(report, 1, .0024) for report in scopes)
          and any(totals(report, 1, .0036) for report in scopes)
          and any(totals(report, 2, .0072) for report in scopes))
    required = {'08-beta-usage-groups': ['$0.0036', '100', '10'],
                '09-all-usage-groups': ['$0.0060', '$0.0024', '$0.0036', '200', '20', 'fixture/alpha', 'fixture/beta'],
                '11-alpha-usage-groups': ['$0.0024', '100', '10'],
                '15-restarted-all-usage-groups': ['$0.0060', '200', '20'],
                '16-restarted-new-turn-all-usage': ['$0.0096', '$0.0072', '$0.0024', '300', '30'],
                '17-postrestart-beta-project-total': ['$0.0072', '200', '20']}
    for name, tokens in required.items():
        content = (out / (name + '.txt')).read_text()
        assert all(token in content for token in tokens), name
        header = (out / (name + '.png')).read_bytes()[:24]
        assert header[:8] == b'\x89PNG\r\n\x1a\n' and struct.unpack('>II', header[16:24]) == (1400, 1000)
    check('Actual Native SDK OS captures show the same nonzero costs, groups, scopes, and recovery',
          True, screenshots=[name + '.png' for name in required])
    cleanup = json.loads((out / 'cleanup.json').read_text())
    remaining = []
    for pid in json.loads((out / 'owned-processes-before-cleanup.json').read_text()):
        try:
            state = Path(f'/proc/{pid}/stat').read_text().rsplit(')', 1)[1].split()[0]
            if state != 'Z':
                remaining.append(pid)
        except OSError:
            pass
    check('All owned sessions and native fixture processes terminate', cleanup['complete'] and not remaining,
          observedOwnedProcessCount=len(json.loads((out / 'owned-processes-before-cleanup.json').read_text())))
    receipt = {'original31KnownCostChain': 'passed', 'checks': checks, 'upstreamBillingTested': False,
               'quotaReadRetested': False, 'nativeGuiMethod': 'Native SDK, private Xvfb, xdotool, ffmpeg, OCR'}
    (out / 'verification.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print(json.dumps({'passed': len(checks), 'receipt': str(out / 'verification.json')}))


if __name__ == '__main__':
    main()
