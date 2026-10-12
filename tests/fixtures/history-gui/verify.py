#!/usr/bin/env python3
"""Verify external receipts from the actual Pi/native GUI history fixture."""
import argparse
import base64
import hashlib
import json
from pathlib import Path
import subprocess


def verify(out, require_restore=False):
    marker = 'COMPLEX_RECOVERY_20261003'
    requests = json.loads((out / 'model-requests.json').read_text())
    complex_requests = [request for request in requests if marker in request['text']]
    assert len(complex_requests) == 2, 'Expected original and unchanged resend'
    original, resend = complex_requests
    assert original == resend, 'Actual native model requests differ'
    assert len(original['userHistoryTexts']) == 2
    assert original['userHistoryTexts'][0] == 'HISTORY_FIRST_20261003'
    assert 'LATER_MESSAGE_20261003' not in str(original)
    assert 'FIRST_NATIVE_RESPONSE' in original['text']
    assert 'Attached file: fixture.txt\nExact attached text payload.\n' in original['text']
    assert len(original['images']) == 1
    image = base64.b64decode(original['images'][0]['image_url']['url'].split(',', 1)[1])
    assert image == (out / 'inputs/fixture.png').read_bytes()
    pdf = next((out / 'home/.local/state/swarmie/workspace/assets').glob('*/fixture.pdf'))
    assert pdf.read_bytes() == (out / 'inputs/fixture.pdf').read_bytes()
    native_users = []
    for journal in (out / 'sessions').glob('*.jsonl'):
        rows = [json.loads(line) for line in journal.read_text().splitlines()]
        selected = [row for row in rows if row['type'] == 'message.user' and marker in str(row['data'])]
        if selected:
            native_users.append((journal.stem, selected[0]))
    assert len(native_users) == 2
    assert native_users[0][1]['data'] == native_users[1][1]['data']
    cli = json.loads((out / 'external-cli.json').read_text())
    assert hashlib.sha256(Path(cli['source']).read_bytes()).hexdigest() == cli['sourceSha256']
    cleanup = json.loads((out / 'descendant-cleanup.json').read_text())
    assert not cleanup['remainingLive'] and cleanup['localModelServerStopped']
    assert not cleanup['modelErrors']
    assert json.loads((out / 'cleanup.json').read_text())['complete']
    result = {
        'actualModelRequestsIdentical': True, 'canonicalUserContentIdentical': True,
        'sessionIds': [item[0] for item in native_users], 'sourceUnchanged': True,
        'historyUserCount': 2, 'laterTurnExcluded': True,
        'imageBytes': len(image), 'imageSha256': hashlib.sha256(image).hexdigest(),
        'pdfBytes': pdf.stat().st_size, 'pdfSha256': hashlib.sha256(pdf.read_bytes()).hexdigest(),
        'exactTextAttachmentAndQuote': True, 'ownedProcessesCleaned': True,
    }
    if require_restore:
        restoration = json.loads((out / 'restore-verified.json').read_text())
        tree = Path(restoration['tree'])
        assert (tree / 'one.txt').read_text() == 'Before native checkpoint.\n'
        assert (out / 'project/one.txt').read_text() == 'Before native checkpoint.\n'
        head = subprocess.check_output(['git', '-C', str(tree), 'rev-parse', 'HEAD'], text=True).strip()
        assert head == restoration['head']
        assert any('WRITE_CHECKPOINT_20261003' in request['text'] for request in requests)
        result['explicitFileRestore'] = restoration
    (out / 'verification.json').write_text(json.dumps(result, indent=2) + '\n')
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('artifacts', type=Path)
    parser.add_argument('--require-restore', action='store_true')
    args = parser.parse_args()
    print(json.dumps(verify(args.artifacts.resolve(), args.require_restore)))


if __name__ == '__main__':
    main()
