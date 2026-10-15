#!/usr/bin/env python3
"""Observe private fixture launch options before delegating to the native peer."""
import json
import os
from pathlib import Path
import runpy
import sys

capture = Path(os.environ['SWARMIE_PROVIDER_CAPTURE'])
mode = sys.argv[1]
keys = ['OPENAI_API_KEY', 'ANTHROPIC_API_KEY', 'XAI_API_KEY', 'GEMINI_API_KEY',
        'GROK_HOME', 'GROK_AUTH_PATH', 'PI_CODING_AGENT_DIR', 'CLAUDE_CONFIG_DIR', 'CODEX_HOME']
capture.write_text(json.dumps({'args': sys.argv[2:], 'env': {key: os.environ.get(key) for key in keys}}))
source = sys.stdin
class ObservedInput:
    def __iter__(self):
        for line in source:
            with capture.with_suffix('.requests.jsonl').open('a') as target:
                target.write(line)
            yield line
sys.stdin = ObservedInput()
if mode == 'grok':
    sys.argv[1] = 'acp'
runpy.run_path(str(Path(__file__).resolve().parents[4] / 'tests/fixtures/agent.py'), run_name='__main__')
