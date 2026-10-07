#!/usr/bin/env python3
"""Verify terminal-to-native opening on an isolated X11 system WebView surface."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time
import urllib.parse

sys.dont_write_bytecode = True


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=Path(__file__).resolve().parents[3])
    parser.add_argument('--artifacts', type=Path, required=True)
    parser.add_argument('--visual-only', action='store_true', help='Save captures for manual review when OCR is unavailable')
    args = parser.parse_args()
    root, out = args.root.resolve(), args.artifacts.resolve()
    if out == root or root in out.parents:
        parser.error('Artifacts must be outside the repository')
    spec = importlib.util.spec_from_file_location('desktop_acceptance', root / 'tests/fixtures/desktop-gui/acceptance.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    desktop = module.Desktop(root, out)
    project = out / 'project with spaces & query'
    desktop.project.rename(project)
    desktop.project = project
    opened_groups = []
    receipts = []
    try:
        desktop.start()
        desktop.rpc('session/metadata', {'sessionId': desktop.session, 'title': 'Terminal selected session'})
        wrong = desktop.rpc('session/create', {'harness': 'mock', 'cwd': str(project), 'config': {}})
        desktop.rpc('session/metadata', {'sessionId': wrong['id'], 'title': 'Unselected session'})
        desktop.stop(desktop.host)
        bus = desktop.child(['dbus-daemon', '--config-file=' + str(out / 'dbus.conf'), '--print-address=1', '--nofork'], pipe=True)
        desktop.env['DBUS_SESSION_BUS_ADDRESS'] = 'unix:' + desktop.ready(bus, 'unix:')
        package = out / 'package/bin'
        package.mkdir(parents=True)
        packaged_cli = package / 'mpai-desktop_services'
        os.link(root / 'target/debug/mpai', packaged_cli)
        (package / 'mpai-desktop').symlink_to(root / 'apps/desktop/zig-out/bin/mpai-desktop')
        for mode, executable, extra in [
            ('explicit-host', root / 'target/debug/mpai', ['--host', str(root / 'apps/desktop/zig-out/bin/mpai-desktop')]),
            ('packaged-sibling', packaged_cli, []),
        ]:
            env = dict(desktop.env)
            # The production CLI must supply both host variables itself.
            env.pop('MPAI_DESKTOP_URL', None)
            env.pop('MPAI_DESKTOP_MANAGED', None)
            command = [str(executable), 'open', '--url', desktop.url + '/?project=old&session=' + wrong['id'], '--project', str(project), '--session', desktop.session, *extra]
            process = subprocess.Popen(command, env=env, cwd=root / 'apps/desktop', stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, start_new_session=True)
            opened_groups.append(process.pid)
            stdout, stderr = process.communicate(timeout=10)
            assert process.returncode == 0, stderr
            query = urllib.parse.parse_qs(urllib.parse.urlsplit(stdout.strip()).query)
            assert query['project'] == [str(project)]
            assert query['session'] == [desktop.session]
            deadline = time.monotonic() + 20
            while time.monotonic() < deadline:
                windows = subprocess.run(['xdotool', 'search', '--onlyvisible', '--name', '^mpai$'], env=env, capture_output=True, text=True)
                if windows.returncode == 0:
                    for window in windows.stdout.splitlines():
                        found = subprocess.run(['xdotool', 'getwindowpid', window], env=env, capture_output=True, text=True)
                        try:
                            owned = found.returncode == 0 and os.getpgid(int(found.stdout.strip())) == process.pid
                        except (ValueError, ProcessLookupError):
                            owned = False
                        if owned:
                            desktop.window = window
                            break
                    else:
                        time.sleep(0.1)
                        continue
                    break
                time.sleep(0.1)
            else:
                raise RuntimeError('The terminal opener did not launch a visible native host')
            pid = int(desktop.x('getwindowpid', desktop.window).strip())
            assert os.getpgid(pid) == process.pid
            desktop.x('windowsize', desktop.window, '1280', '900')
            desktop.x('windowfocus', '--sync', desktop.window)
            time.sleep(3)
            capture = desktop.capture('open-' + mode)
            if not args.visual_only:
                assert capture['ocr'], 'OCR is unavailable; install tesseract or use --visual-only and inspect the saved screenshots'
                assert 'Native desktop acceptance fixture' in capture['ocr'], capture['ocr']
                assert 'Terminal selected session' in capture['ocr'], capture['ocr']
            receipts.append({'mode': mode, 'command': command, 'cliExitCode': process.returncode, 'url': stdout.strip(), 'nativePid': pid, 'window': desktop.window, 'capture': capture, 'project': str(project), 'sessionId': desktop.session})
            (out / 'native-open-receipts.json').write_text(json.dumps(receipts, indent=2) + '\n', encoding='utf-8')
            os.killpg(process.pid, signal.SIGTERM)
            opened_groups.remove(process.pid)
            deadline = time.monotonic() + 5
            while time.monotonic() < deadline:
                visible = subprocess.run(['xdotool', 'search', '--onlyvisible', '--name', '^mpai$'], env=env, capture_output=True)
                if visible.returncode != 0:
                    break
                time.sleep(0.1)
        print(json.dumps({'actualNativeHost': True, 'explicitHost': True, 'packagedSibling': True, 'projectAndSessionSelected': 'manual review required' if args.visual_only else True, 'artifacts': str(out)}))
    finally:
        for group in opened_groups:
            try:
                os.killpg(group, signal.SIGTERM)
            except ProcessLookupError:
                pass
        desktop.close()


if __name__ == '__main__':
    main()
