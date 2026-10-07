#!/usr/bin/env python3
"""Independent native-command peer; no real screen data is accessed."""
import base64
import json
import pathlib
import sys
name = pathlib.Path(sys.argv[0]).name
args = sys.argv[1:]
with pathlib.Path(__file__).resolve().with_suffix('.jsonl').open('a') as output:
    output.write(json.dumps({'program':name,'args':args})+'\n')
if name == 'wmctrl':
    print('0x01234567 0 4242 10 20 800 600 fixture-host Selected fixture window')
elif name == 'xprop':
    print('_NET_WM_NAME(UTF8_STRING) = "Selected fixture window"\n_NET_WM_PID(CARDINAL) = 4242')
elif name == 'import':
    pathlib.Path(args[-1]).write_bytes(base64.b64decode('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jr/8AAAAASUVORK5CYII='))
elif name == 'python3':
    assert args[-1] == '4242'
    print(json.dumps([{'name':'Selected fixture element','role':'text'}]))
else:
    raise SystemExit('unexpected window peer operation')
