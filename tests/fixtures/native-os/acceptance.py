#!/usr/bin/env python3
"""Verify real X11 window listing/capture and AT-SPI selected-process context through swarmie."""
import argparse
import base64
import collections
import hashlib
import json
import os
from pathlib import Path
import select
import signal
import struct
import subprocess
import sys
import time
import urllib.request


def ready(process, prefix, out, timeout=20):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        if process.poll() is not None:
            raise RuntimeError('Owned process exited before readiness')
        if select.select([process.stdout], [], [], .2)[0]:
            line = process.stdout.readline()
            with (out / 'readiness.log').open('a') as log:
                log.write(line)
            if line.startswith(prefix) and (prefix or line.strip().isdigit()):
                return line[len(prefix):].strip()
    raise TimeoutError('Owned process readiness')


def wait(check, label):
    end = time.monotonic() + 25
    while time.monotonic() < end:
        value = check()
        if value:
            return value
        time.sleep(.15)
    raise TimeoutError(label)


def live_group(group):
    result = []
    for path in Path('/proc').glob('[0-9]*/stat'):
        try:
            fields = path.read_text().rsplit(')', 1)[1].split()
            if int(fields[2]) == group and fields[0] != 'Z':
                result.append(int(path.parent.name))
        except (OSError, ValueError, IndexError):
            pass
    return result


def run_inside(root, out):
    fixture = Path(__file__).resolve().parent
    vendor = fixture / 'vendor'
    env = os.environ.copy()
    env.update(PATH=str(vendor / 'bin') + ':/usr/bin:/bin',
               LD_LIBRARY_PATH=str(vendor / 'usr/lib/x86_64-linux-gnu'),
               PYTHONPATH=str(vendor / 'usr/lib/python3/dist-packages'),
               MAGICK_CONFIGURE_PATH=str(vendor / 'etc/ImageMagick-6'),
               MAGICK_CODER_MODULE_PATH=str(vendor / 'usr/lib/x86_64-linux-gnu/ImageMagick-6.9.12/modules-Q16/coders'),
               XDG_DATA_DIRS=str(vendor / 'usr/share') + ':/usr/local/share:/usr/share',
               GTK_A11Y='atspi', NO_AT_BRIDGE='0')
    processes, logs, receipts = [], [], []
    report = {'checks': {}, 'nativeBackend': 'x11/at-spi', 'systemPackagesInstalled': False,
              'daemonSha256': hashlib.sha256((root / 'target/debug/swarmie').read_bytes()).hexdigest()}

    def child(command, name, pipe=False):
        log = (out / (name + '.log')).open('a')
        logs.append(log)
        process = subprocess.Popen(command, env=env, stdin=subprocess.DEVNULL,
                                   stdout=subprocess.PIPE if pipe else log, stderr=subprocess.STDOUT if pipe else log,
                                   text=True, start_new_session=True)
        processes.append(process)
        return process

    def rpc(method, params=None, expected_error=False):
        req = urllib.request.Request(url, json.dumps({'jsonrpc': '2.0', 'id': len(receipts) + 1,
            'method': method, 'params': params or {}}).encode(), {'Content-Type': 'application/json',
            'Authorization': 'Bearer controlled-os-fixture'})
        with urllib.request.urlopen(req, timeout=35) as response:
            value = json.load(response)
        receipts.append({'method': method, 'params': params or {}, 'response': value})
        if expected_error:
            assert 'error' in value, value
            return value['error']
        if 'error' in value:
            raise RuntimeError(json.dumps(value['error']))
        return value['result']

    def invoke(command, arguments=None, expected_error=False):
        return rpc('extension/invoke', {'extensionId': 'workspace', 'command': command,
                                       'arguments': arguments or {}}, expected_error)

    try:
        xvfb = child(['/usr/bin/Xvfb', '-displayfd', '1', '-screen', '0', '1280x900x24', '-nolisten', 'tcp'], 'xvfb', True)
        display = ':' + ready(xvfb, '', out)
        env.update(DISPLAY=display, SWARMIE_OS_PRIVATE_DISPLAY=display)
        report['display'] = display
        report['privateDbus'] = os.environ['DBUS_SESSION_BUS_ADDRESS']
        child([str(vendor / 'bin/openbox'), '--config-file', str(vendor / 'etc/xdg/openbox/rc.xml')], 'openbox')
        wait(lambda: subprocess.run(['/usr/bin/xprop', '-root', '_NET_SUPPORTING_WM_CHECK'], env=env,
             text=True, capture_output=True).stdout.find('window id #') >= 0, 'private window manager')
        address = subprocess.check_output(['/usr/bin/gdbus', 'call', '--session', '--dest', 'org.a11y.Bus',
                     '--object-path', '/org/a11y/bus', '--method', 'org.a11y.Bus.GetAddress'], env=env, text=True)
        report['accessibilityBus'] = address.strip()
        decoy = child(['/usr/bin/python3', str(fixture / 'window.py'), 'decoy'], 'decoy')
        selected = child(['/usr/bin/python3', str(fixture / 'window.py'), 'selected'], 'selected')
        daemon = child([str(root / 'target/debug/swarmie'), '--state-dir', str(out / 'sessions'), 'serve',
                        '--http', '127.0.0.1:0', '--token', 'controlled-os-fixture', '--workbench'], 'daemon', True)
        url = ready(daemon, 'HTTP listening on ', out)
        rpc('runtime/initialize')
        capabilities = invoke('platform.capabilities')
        integrations = {item['id']: item['available'] for item in capabilities['integrations']}
        assert all(integrations[name] for name in ['wmctrl', 'xprop', 'import', 'python3']), capabilities
        windows = wait(lambda: [window for window in invoke('windows.list')['windows']
                               if window['title'] == 'SWARMIE controlled native OS fixture'], 'actual GTK window listing')
        window = windows[0]
        assert window['pid'] == selected.pid, window
        assert window['bounds']['width'] == 640 and window['bounds']['height'] == 360, window
        assert any(w['pid'] == decoy.pid for w in invoke('windows.list')['windows'])
        report['window'] = window
        report['checks']['actualOwnedWindowListAndPid'] = True
        subprocess.run([str(vendor / 'bin/wmctrl'), '-ia', window['id']], env=env, check=True)
        time.sleep(.3)
        captured = invoke('windows.capture', {'windowId': window['id']})
        assert captured['mime'] == 'image/png', captured
        asset = invoke('assets.read', {'assetId': captured['assetId']})
        image = base64.b64decode(asset['data'])
        assert image.startswith(b'\x89PNG\r\n\x1a\n')
        dimensions = struct.unpack('>II', image[16:24])
        assert dimensions == (640, 360), dimensions
        path = out / 'selected-window.png'
        path.write_bytes(image)
        pixels = subprocess.check_output([str(vendor / 'bin/convert'), str(path), '-depth', '8', 'rgba:-'], env=env)
        assert len(pixels) == 640 * 360 * 4
        colors = collections.Counter(zip(*[iter(pixels)] * 4))
        assert colors[(17, 68, 170, 255)] > 20_000 and len(colors) > 25, colors.most_common(5)
        report['capture'] = {'width': dimensions[0], 'height': dimensions[1], 'bytes': len(image),
                             'sha256': hashlib.sha256(image).hexdigest(), 'knownBackgroundPixels': colors[(17, 68, 170, 255)],
                             'uniqueColors': len(colors)}
        report['checks']['actualSelectedWindowPngAndContent'] = True
        def accessible():
            value = invoke('windows.context', {'windowId': window['id']})
            if value.get('accessibilityError'):
                raise AssertionError(value)
            items = json.loads(value['accessibility'])
            return value if any(item['name'] == 'Controlled action button' for item in items) else None
        context = wait(accessible, 'actual native accessible controls')
        items = json.loads(context['accessibility'])
        names = {item['name'] for item in items}
        assert {'SWARMIE controlled native OS fixture', 'CONTROLLED_VISIBLE_TEXT_20261003', 'Controlled action button', 'Controlled entry field'} <= names, items
        assert not any('DECOY' in name or 'Decoy' in name for name in names), names
        assert any(item['name'] == 'Controlled action button' and item['role'] == 'push button' for item in items)
        report['accessibility'] = context
        report['checks']['realSelectedProcessAtSpiNamesRolesAndText'] = True
        for command in ['windows.capture', 'windows.context']:
            for bad in ['-root', '0xZZ', '1;touch /tmp/not-permitted', '', '0x']:
                error = invoke(command, {'windowId': bad}, True)
                assert error['code'] == -32602, error
        report['checks']['malformedWindowIdentifiersRejected'] = True
        daemon.send_signal(signal.SIGINT)
        assert daemon.wait(timeout=15) == 0
    except Exception as error:
        report['failure'] = repr(error)
        raise
    finally:
        for process in reversed(processes):
            if process.poll() is None:
                os.killpg(process.pid, signal.SIGTERM)
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL)
                    process.wait(timeout=5)
        for log in logs:
            log.close()
        report['checks']['ownedChildrenExited'] = all(process.poll() is not None for process in processes)
        (out / 'result.json').write_text(json.dumps(report, indent=2) + '\n')
        (out / 'rpc-receipts.json').write_text(json.dumps(receipts, indent=2) + '\n')
    print(json.dumps(report, indent=2))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=Path(__file__).resolve().parents[3])
    parser.add_argument('--artifacts', type=Path, required=True)
    parser.add_argument('--inside-private-session', action='store_true', help=argparse.SUPPRESS)
    args = parser.parse_args()
    root, out = args.root.resolve(), args.artifacts.resolve()
    if out == root or root in out.parents:
        parser.error('Artifacts must be outside the repository')
    if args.inside_private_session:
        if os.environ.get('SWARMIE_OS_PRIVATE_SESSION') != '1' or not os.environ.get('DBUS_SESSION_BUS_ADDRESS'):
            parser.error('Only the fixture parent may create the private graphical session')
        run_inside(root, out)
        return
    out.mkdir(parents=True, exist_ok=False)
    home, runtime = out / 'home', out / 'runtime'
    home.mkdir()
    runtime.mkdir(mode=0o700)
    env = {key: os.environ[key] for key in ['PATH', 'USER', 'LOGNAME', 'LANG'] if key in os.environ}
    env.update(HOME=str(home), XDG_CONFIG_HOME=str(out / 'xdg-config'), XDG_DATA_HOME=str(out / 'xdg-data'),
               XDG_CACHE_HOME=str(out / 'xdg-cache'), XDG_STATE_HOME=str(out / 'xdg-state'),
               XDG_RUNTIME_DIR=str(runtime), SWARMIE_OS_PRIVATE_SESSION='1')
    process = subprocess.Popen(['/usr/bin/dbus-run-session', '--', '/usr/bin/python3', str(Path(__file__).resolve()),
                               '--root', str(root), '--artifacts', str(out), '--inside-private-session'], env=env, start_new_session=True)
    try:
        code = process.wait(timeout=180)
    finally:
        try:
            os.killpg(process.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        if process.poll() is None:
            process.wait(timeout=5)
        deadline = time.monotonic() + 5
        while live_group(process.pid) and time.monotonic() < deadline:
            time.sleep(.05)
        remaining = live_group(process.pid)
        if remaining:
            os.killpg(process.pid, signal.SIGKILL)
            wait(lambda: not live_group(process.pid), 'private D-Bus services cleanup')
        result = out / 'result.json'
        if result.exists():
            report = json.loads(result.read_text())
            report['checks']['privateDbusServicesExited'] = not live_group(process.pid)
            result.write_text(json.dumps(report, indent=2) + '\n')
    raise SystemExit(code)


if __name__ == '__main__':
    main()
