#!/usr/bin/env python3
"""Independent native adb argument peer with real binary screenshot output."""
import base64
import pathlib
import sys
args=sys.argv[1:]
if args==['devices','-l']:
    print('List of devices attached\nemulator-5554 device product:fixture model:Fixture_Model')
elif args[-3:]==['exec-out','screencap','-p']:
    sys.stdout.buffer.write(base64.b64decode('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAusB9Y9ZrKEAAAAASUVORK5CYII='))
elif 'am' in args and 'start' in args:
    print('Starting fixture settings')
else:
    raise SystemExit('unsupported adb fixture request')
