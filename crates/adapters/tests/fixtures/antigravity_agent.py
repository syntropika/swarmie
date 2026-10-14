#!/usr/bin/env python3
"""Independent peer for the documented Antigravity headless wire."""
import json
import os
import sys
import time
from pathlib import Path


def send(value):
    print(json.dumps(value), flush=True)


if 'models' in sys.argv:
    send({'command': {'data': {'models': [{'id': 'fixture-model', 'label': 'Fixture model'}]}}})
    sys.exit(0)
if os.environ.get('SWARMIE_FAIL_INIT') == '1':
    send({'event': 'result', 'result': {'status': 'ERROR', 'error': 'authentication required'}})
    sys.exit(1)
args = sys.argv[1:]
session = args[args.index('--conversation') + 1] if '--conversation' in args else 'native-conversation'
if os.environ.get('SWARMIE_WRONG_RESUME') == '1' and '--conversation' in args:
    session = 'wrong-conversation'
with Path('launches.jsonl').open('a') as log:
    log.write(json.dumps({'args': args, 'pid': os.getpid(), 'session': session}) + '\n')
init = {'cwd': str(Path.cwd()), 'tools': ['view_file', 'run_command'], 'permission_mode': 'request-review'}
if '--model' in args:
    init['model'] = args[args.index('--model') + 1]
if '--json-schema' in args:
    init['json_schema'] = json.loads(args[args.index('--json-schema') + 1])
send({'event': 'init', 'conversation_id': session, 'init': init})
for line in sys.stdin:
    prompt = json.loads(line)
    assert prompt['event'] == 'user'
    assert isinstance(prompt['message']['content'], str)
    text = prompt['message']['content']
    send({'event': 'step_update', 'step_update': {'step_index': 2, 'state': 'ACTIVE', 'step_type': 'tool', 'tool_name': 'view_file', 'tool_info': {'parameters': {'TargetFile': 'hello.txt'}}}})
    if text == 'slow':
        time.sleep(100)
        continue
    send({'event': 'step_update', 'step_update': {'step_index': 2, 'state': 'DONE', 'step_type': 'tool', 'tool_info': {'output': 'contents'}}})
    send({'event': 'step_update', 'step_update': {'step_index': 3, 'state': 'DONE', 'step_type': 'subagent', 'subagent_info': {'subagents': [{'conversation_id': 'child-conversation', 'type_name': 'reviewer', 'role': 'Review changes'}]}}})
    settings = Path.home() / '.gemini/antigravity-cli/settings.json'
    receipt = json.dumps({'prompt': prompt, 'args': args, 'env': os.environ.get('SWARMIE_NATIVE_ENV'), 'pid': os.getpid(), 'session': session, 'home': str(Path.home()), 'geminiHome': os.environ.get('GEMINI_HOME'), 'settings': json.loads(settings.read_text()) if settings.is_file() else None, 'geminiKeyPresent': bool(os.environ.get('GEMINI_API_KEY'))})
    send({'event': 'step_update', 'step_update': {'step_index': 4, 'state': 'ACTIVE', 'step_type': 'agent_response', 'text_delta': receipt}})
    send({'event': 'step_update', 'step_update': {'step_index': 4, 'state': 'DONE', 'step_type': 'agent_response', 'usage': {'input_tokens': 100, 'output_tokens': 7, 'cache_read_tokens': 50, 'total_tokens': 107}}})
    result = {'status': 'SUCCESS', 'response': receipt, 'usage': {'input_tokens': 70000, 'output_tokens': 7777, 'cache_read_tokens': 50000, 'total_tokens': 77777}}
    if '--json-schema' in args:
        result['structured_output'] = {'ok': True}
    send({'event': 'result', 'result': result})
