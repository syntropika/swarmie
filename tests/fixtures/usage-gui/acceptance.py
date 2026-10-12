#!/usr/bin/env python3
"""Drive an isolated Native SDK usage dashboard using real Pi and configured fixture rates."""
import argparse
import hashlib
from http.server import ThreadingHTTPServer
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


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=Path(__file__).resolve().parents[3])
    parser.add_argument('--artifacts', type=Path, required=True)
    args = parser.parse_args()
    root, out = args.root.resolve(), args.artifacts.resolve()
    if out == root or root in out.parents:
        parser.error('Keep artifacts outside the repository')
    out.mkdir(parents=True, exist_ok=False)
    helper = load('native_desktop', root / 'tests/fixtures/desktop-gui/acceptance.py')
    peer = load('native_usage_peer', root / 'tests/fixtures/resources-acceptance/acceptance.py')
    desktop = helper.Desktop(root, out)
    vendor = root / 'tests/fixtures/resources-gui/vendor'
    pi = root / 'tests/fixtures/harnesses/node_modules/.bin/pi'
    desktop.env.update(PATH=str(vendor / 'usr/bin') + ':' + str(pi.parent) + ':' + os.environ['PATH'],
                       LD_LIBRARY_PATH=str(vendor / 'usr/lib/x86_64-linux-gnu'),
                       TESSDATA_PREFIX=str(vendor / 'usr/share/tesseract-ocr/5/tessdata'),
                       WEBKIT_DISABLE_COMPOSITING_MODE='1', GTK_A11Y='none')
    desktop.env['USAGE_FIXTURE_KEY'] = 'synthetic-' + uuid.uuid4().hex
    server = ThreadingHTTPServer(('127.0.0.1', 0), peer.Peer)
    server.daemon_threads = True
    server.receipts, server.errors, server.workspace = [], [], desktop.project
    threading.Thread(target=server.serve_forever, daemon=True).start()
    config = out / 'pi-agent'
    config.mkdir(mode=0o700)
    desktop.env["PI_CODING_AGENT_DIR"] = str(config)
    models = [{'id': model, 'name': model, 'reasoning': False, 'input': ['text'],
               'contextWindow': 32768, 'maxTokens': 1024,
               'cost': {'input': rate, 'output': rate * 2, 'cacheRead': 0, 'cacheWrite': 0}}
              for model, rate in [('alpha', 20), ('beta', 30)]]
    (config / 'models.json').write_text(json.dumps({'providers': {'fixture': {
        'baseUrl': f'http://127.0.0.1:{server.server_port}/v1', 'api': 'openai-completions',
        'apiKey': 'USAGE_FIXTURE_KEY', 'models': models}}}, indent=2) + '\n')
    (config / 'settings.json').write_text(json.dumps({'compaction': {'enabled': False}, 'retry': {'enabled': False}}) + '\n')
    for command in [['git', 'init', '-b', 'main', str(desktop.project)], ['git', '-C', str(desktop.project), 'add', '.'],
        ['git', '-C', str(desktop.project), '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid',
         'commit', '-m', 'Initialize controlled usage project']]:
        subprocess.run(command, check=True, capture_output=True)
    actions = []
    daemon = None
    assets = out / 'desktop-assets'
    shutil.copytree(root / 'apps/desktop/dist', assets)
    fingerprints = {str(path.relative_to(root)): hashlib.sha256(path.read_bytes()).hexdigest() for path in
        [root / 'target/debug/swarmie', root / 'apps/desktop/zig-out/bin/swarmie-desktop', *sorted((root / 'apps/desktop/dist').rglob('*.js'))]}
    (out / 'build.json').write_text(json.dumps({'files': fingerprints,
        'nativeVersion': subprocess.check_output([str(pi), '--version'], text=True, env=desktop.env).strip(),
        'configuredRatesPerMillion': models, 'upstreamBillingTested': False}, indent=2) + '\n')
    def launch_daemon():
        nonlocal daemon
        daemon = desktop.child([str(root / 'target/debug/swarmie'), '--state-dir', str(out / 'sessions'), 'serve', '--http',
            '127.0.0.1:0', '--desktop-assets', str(assets), '--workbench', '--git-worktrees', str(out / 'worktrees'),
            '--resources-root', str(out / 'resources')], pipe=True)
        desktop.url = desktop.ready(daemon, 'HTTP listening on ').removesuffix('/rpc')
        desktop.env['SWARMIE_DESKTOP_URL'] = desktop.url + '/?project=' + urllib.parse.quote(str(desktop.project))
    try:
        launch_daemon()
        desktop.rpc('runtime/initialize')
        opened = desktop.rpc('extension/invoke', {'extensionId': 'git', 'command': 'project.open', 'arguments': {'path': str(desktop.project)}})
        worktree = desktop.rpc('extension/invoke', {'extensionId': 'git', 'command': 'worktree.create',
            'arguments': {'projectId': opened['id'], 'branch': 'usage-scope'}})
        for cwd, model in [(str(desktop.project), 'alpha'), (worktree['path'], 'beta')]:
            desktop.rpc('extension/invoke', {'extensionId': 'resources', 'command': 'project.set', 'arguments': {
                'cwd': cwd, 'harness': 'pi', 'model': 'fixture/' + model, 'config': {'executable': str(pi),
                    'args': ['--mode', 'rpc', '--provider', 'fixture', '--no-builtin-tools', '--no-skills', '--no-prompt-templates']}}})
        xvfb = desktop.child(['Xvfb', '-displayfd', '1', '-screen', '0', '1600x1100x24', '-nolisten', 'tcp'], pipe=True)
        desktop.display = ':' + desktop.ready(xvfb, '')
        desktop.env.update(DISPLAY=desktop.display, SWARMIE_DESKTOP_MANAGED='1', XDG_DATA_HOME=str(out / 'xdg-data'),
            XDG_CACHE_HOME=str(out / 'xdg-cache'))
        (out / 'dbus.conf').write_text('<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN" "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd"><busconfig><type>session</type><listen>unix:tmpdir=/tmp</listen><policy context="default"><allow send_destination="*"/><allow receive_sender="*"/><allow own="*"/></policy></busconfig>')
        desktop.launch_host()
        desktop.x('windowsize', desktop.window, '1400', '1000')
        print(json.dumps({'ready': {'url': desktop.url, 'display': desktop.display, 'window': desktop.window,
            'project': str(desktop.project), 'worktree': worktree, 'ratesPerMillion': models}}), flush=True)
        for raw in sys.stdin:
            request = json.loads(raw)
            try:
                if request['action'] == 'capture':
                    time.sleep(.4)
                    geometry = dict(line.split('=', 1) for line in desktop.x('getwindowgeometry', '--shell', desktop.window).splitlines() if '=' in line)
                    path = out / (request['name'] + '.png')
                    subprocess.run(['ffmpeg', '-nostdin', '-loglevel', 'error', '-y', '-f', 'x11grab', '-video_size',
                        geometry['WIDTH'] + 'x' + geometry['HEIGHT'], '-i', desktop.display + '+' + geometry['X'] + ',' + geometry['Y'],
                        '-frames:v', '1', str(path)], env=desktop.env, check=True)
                    ocr = subprocess.check_output([str(vendor / 'usr/bin/tesseract'), str(path), 'stdout'], env=desktop.env,
                        text=True, stderr=subprocess.DEVNULL)
                    (out / (request['name'] + '.txt')).write_text(ocr)
                    result = {'screenshot': str(path), 'ocr': ocr}
                elif request['action'] == 'restart-all':
                    desktop.stop(desktop.host)
                    desktop.stop(daemon)
                    launch_daemon()
                    desktop.launch_host()
                    desktop.x('windowsize', desktop.window, '1400', '1000')
                    result = {'restarted': True, 'url': desktop.url, 'window': desktop.window}
                else:
                    result = desktop.action(request)
                response = {'request': request, 'result': result}
            except Exception as error:
                response = {'request': request, 'error': str(error)}
            actions.append(response)
            (out / 'actions.json').write_text(json.dumps(actions, indent=2) + '\n')
            (out / 'model-requests.json').write_text(json.dumps(server.receipts, indent=2) + '\n')
            print(json.dumps(response), flush=True)
    finally:
        desktop.close()
        server.shutdown()
        server.server_close()
        (out / 'model-requests.json').write_text(json.dumps(server.receipts, indent=2) + '\n')
        (out / 'model-errors.json').write_text(json.dumps(server.errors, indent=2) + '\n')


if __name__ == '__main__':
    main()
