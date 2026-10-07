#!/usr/bin/env python3
"""Argument-only repository publishing peer; performs no network operation."""
import json
import pathlib
import sys
args = sys.argv[1:]
assert args[:2] == ['repo','create']
with pathlib.Path(__file__).with_suffix('.jsonl').open('a') as output:
    output.write(json.dumps(args)+'\n')
print('https://github.example.test/fixture/project')
