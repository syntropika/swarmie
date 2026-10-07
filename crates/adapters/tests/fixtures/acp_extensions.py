#!/usr/bin/env python3
"""Independent peers for documented Cursor, Grok and Kiro extension contracts."""
import json
import sys

mode = sys.argv[1]
replies = []
prompt_id = None
model = 'model-a'


def send(value):
    print(json.dumps(value), flush=True)


def request(method, params, ident):
    send({'jsonrpc': '2.0', 'id': ident, 'method': method, 'params': params})


def update(method, params):
    send({'jsonrpc': '2.0', 'method': method, 'params': params})


def options():
    return [{'id': 'model', 'name': 'Model', 'category': 'model', 'type': 'select', 'currentValue': model, 'options': [{'group': 'Models', 'options': [{'value': 'model-a', 'name': 'Model A'}, {'value': 'model-b', 'name': 'Model B'}]}]}]


def question():
    if mode == 'grok':
        request('_x.ai/ask_user_question', {'sessionId': 'session', 'toolCallId': 'question-tool', 'questions': [{'question': 'Which option?', 'options': [{'label': 'First', 'preview': 'First preview'}, {'label': 'Second'}], 'multiSelect': False}], 'mode': 'default'}, 137)
    else:
        request('cursor/ask_question', {'sessionId': 'session', 'toolCallId': 'question-tool', 'questions': [{'id': 'question-a', 'prompt': 'Which option?', 'options': [{'id': 'option-a', 'label': 'First'}, {'id': 'option-b', 'label': 'Second'}], 'allowMultiple': False}]}, 137)


for line in sys.stdin:
    frame = json.loads(line)
    method = frame.get('method')
    ident = frame.get('id')
    if method == 'initialize':
        send({'jsonrpc': '2.0', 'id': ident, 'result': {'protocolVersion': 1, 'agentInfo': {'name': 'Kiro CLI' if mode == 'kiro' else mode}, 'agentCapabilities': {'loadSession': True}}})
    elif method == 'session/new':
        result = {'sessionId': 'session'}
        if mode == 'kiro':
            result['configOptions'] = options()
            update('_kiro.dev/commands/available', {'sessionId': 'session', 'commands': [{'name': '/context add', 'description': 'Add context', 'meta': {'hint': '<path>'}}]})
        send({'jsonrpc': '2.0', 'id': ident, 'result': result})
    elif method == 'session/set_config_option':
        replies.append(frame)
        assert frame['params']['configId'] == 'model'
        model = frame['params']['value']
        send({'jsonrpc': '2.0', 'id': ident, 'result': {'configOptions': options()}})
    elif method == 'session/set_model':
        send({'jsonrpc': '2.0', 'id': ident, 'error': {'code': -32601, 'message': 'legacy model method is unavailable'}})
    elif method == 'session/prompt':
        prompt_id = ident
        if mode == 'kiro':
            update('session/update', {'sessionId': 'session', 'update': {'sessionUpdate': 'agent_message_chunk', 'content': {'type': 'text', 'text': json.dumps({'model': model, 'requests': replies})}}})
            send({'jsonrpc': '2.0', 'id': prompt_id, 'result': {'stopReason': 'end_turn'}})
        else:
            question()
    elif ident == 137:
        replies.append(frame)
        if mode == 'grok':
            request('_x.ai/exit_plan_mode', {'sessionId': 'session', 'toolCallId': 'plan-tool', 'planContent': '# Plan\nInspect the project.'}, 138)
        else:
            request('cursor/create_plan', {'sessionId': 'session', 'toolCallId': 'plan-tool', 'name': 'Review plan', 'plan': '# Plan\nInspect the project.', 'todos': [{'id': 'todo-a', 'content': 'Inspect files', 'status': 'pending'}]}, 138)
    elif ident == 138:
        replies.append(frame)
        if mode == 'cursor':
            update('cursor/update_todos', {'sessionId': 'session', 'todos': [{'id': 'todo-a', 'content': 'Inspect files', 'status': 'pending'}], 'merge': False})
            update('cursor/update_todos', {'sessionId': 'session', 'todos': [{'id': 'todo-a', 'content': 'Inspect files', 'status': 'completed'}, {'id': 'todo-b', 'content': 'Report', 'status': 'pending'}], 'merge': True})
            update('cursor/task', {'sessionId': 'session', 'toolCallId': 'task-call', 'agentId': 'child-a', 'description': 'Review files', 'durationMs': 14})
        request('unsupported/extension', {'sessionId': 'session'}, 139)
    elif ident == 139:
        replies.append(frame)
        update('session/update', {'sessionId': 'session', 'update': {'sessionUpdate': 'agent_message_chunk', 'content': {'type': 'text', 'text': json.dumps({'replies': replies})}}})
        send({'jsonrpc': '2.0', 'id': prompt_id, 'result': {'stopReason': 'end_turn'}})
