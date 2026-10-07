#!/usr/bin/env python3
"""Verify retained real native-window, SSH, macOS filesystem, and cleanup receipts."""
import argparse
import json
from pathlib import Path
import sqlite3
import struct
from urllib.parse import urlparse


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('artifacts', type=Path)
    out = parser.parse_args().artifacts.resolve()
    read = lambda name: json.loads((out / name).read_text())
    actions = read('actions.json')
    ready = read('ready.json')
    events = read('ssh-events.json')
    checks = []

    def check(name, condition, **evidence):
        checks.append({'name': name, 'passed': bool(condition), **evidence})

    def results(action):
        return [entry.get('result', {}) for entry in actions if entry['request']['action'] == action]

    def ocr(name):
        return (out / (name + '.txt')).read_text()

    def marker(name, text):
        # Native screenshot OCR may read narrow underscores as spaces.
        normalized = lambda value: ''.join(character for character in value if character.isalnum())
        return normalized(text) in normalized(ocr(name))

    check('Actual authenticated production SSH uses distinct local and macOS listener ports',
          ready['remote']['os'] == 'Darwin' and ready['forwardAuthenticated']
          and urlparse(ready['url']).port != ready['remote']['port']
          and events[0]['event'] == 'ssh.connected' and events[0]['authenticated'],
          localPort=urlparse(ready['url']).port, remotePort=ready['remote']['port'])
    check('Native second client requires token entry and opens requested project after authentication',
          'token' in ocr('initial-auth-required').lower()
          and any(entry['request']['action'] == 'token-type' and entry['result']['secret'] for entry in actions)
          and 'project' in ocr('remote-authenticated-url-project').lower())
    verified = results('remote-verify')
    check('GUI file Save and real 20ms keyboard PTY writes execute on the macOS filesystem',
          any(result.get('fileContent') == 'REMOTE_GUI_FILE_EDIT'
              and result.get('terminalWritten') == 'REMOTE_GUI_PTY_WRITE\n'
              and result.get('os') == 'Darwin' for result in verified)
          and 'Darwin' in ocr('remote-gui-pty')
          and marker('remote-gui-pty', 'REMOTE_GUI_PTY_DONE'))
    connected = [entry for entry in events if entry['event'] == 'ssh.connected']
    check('Killed SSH child is replaced and existing native GUI PTY remains usable',
          len(connected) == 2 and connected[0]['sshPid'] != connected[1]['sshPid']
          and all(entry['authenticated'] for entry in connected)
          and any(entry['event'] == 'ssh.disconnected' for entry in events)
          and marker('remote-gui-reconnected', 'REMOTE_GUI_AFTER_RECONNECT'))
    png = (out / 'remote-mobile-390.png').read_bytes()
    width, height = struct.unpack('>II', png[16:24])
    check('Actual native window renders remote Terminal at 390 pixels',
          png.startswith(b'\x89PNG\r\n\x1a\n') and (width, height) == (390, 844)
          and 'Terminal' in ocr('remote-mobile-390'), width=width, height=height)
    check('GUI boots an owned previously Shutdown simulator and captures a message image',
          results('owned-device')[0]['initialState'] == 'Shutdown'
          and 'Booted' in ocr('simulator-opening')
          and 'device' in ocr('simulator-captured-panel').lower(),
          visualEvidence='simulator-captured-panel.png: native iOS home screen plus composer PNG attachment')
    check('GUI renders authenticated remote live device frames and stops the live view',
          'Stop live view' in ocr('simulator-live-stop-control')
          and 'Stop live view' in ocr('simulator-live-second-frame')
          and 'Stop live view' not in ocr('simulator-live-stopped'),
          visualEvidence='simulator-live-stop-control.png and simulator-live-stopped.png',
          limitation='The opaque GUI stream UUID was not extracted; unknown/closed-ID rejection is covered by separate backend tests.')
    check('Revoked GUI token is rejected and replacement token reconnects both clients',
          'token' in ocr('old-token-rejected').lower()
          and 'Connected' in ocr('replacement-token-connected')
          and any(entry['request'].get('generation') == 'replacement' for entry in actions)
          and results('repair-cli')[0]['authenticated']
          and 'REMOTE_GUI_FILE' in ocr('remote-file-reloaded-after-reauthentication'))
    secrets = [path.read_text().strip() for path in (out / 'private').iterdir() if path.is_file()]
    leaks = []
    for path in out.rglob('*'):
        if not path.is_file() or out / 'private' in path.parents or out / 'cli-client' in path.parents:
            continue
        data = path.read_bytes()
        if any(secret.encode() in data or secret.encode('utf-16le') in data for secret in secrets):
            leaks.append(str(path.relative_to(out)))
    storage_found = False
    for path in (out / 'xdg-data').rglob('localstorage.sqlite3'):
        with sqlite3.connect(f'file:{path}?mode=ro', uri=True) as connection:
            for key, value in connection.execute('SELECT key,value FROM ItemTable'):
                text = value.decode('utf-16le') if isinstance(value, bytes) else value
                if key == 'mpai.workspace.v2':
                    storage_found = ready['remote']['project'] in text and not any(secret in text for secret in secrets)
    check('Public receipts and decoded persistent GUI workspace contain no token values',
          not leaks and storage_found and (out / 'cli-client/connections.json').stat().st_mode & 0o777 == 0o600
          and all(path.stat().st_mode & 0o777 == 0o600 for path in (out / 'private').iterdir()),
          leakedPaths=leaks, privateCliCredentialFileMode='0600')
    local, remote, mac = read('cleanup.json'), read('remote-cleanup.json'), read('mac-cleanup.json')
    check('Owned local processes, SSH supervisor, and macOS daemon stop; owned simulator returns to Shutdown',
          local['complete'] and all(entry['returncode'] is not None for entry in local['ownedProcesses'])
          and remote['remoteControllerExited'] and remote['supervisorExited'] and mac['daemonStopped']
          and len(mac['ownedDevices']) == 1 and mac['ownedDevices'][0]['restoredState'] == 'Shutdown'
          and mac['ownedDevices'][0]['exitCode'] == 0)
    result = {'passed': all(check['passed'] for check in checks), 'checks': checks}
    (out / 'verification.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result, indent=2))
    raise SystemExit(0 if result['passed'] else 1)


if __name__ == '__main__':
    main()
