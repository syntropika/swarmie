#!/usr/bin/env python3
"""Deterministic configuration peer; it never invokes a model or native tools."""
import json
import os
import sys

mode = sys.argv[1]
permission = sys.argv[sys.argv.index('--permission-mode') + 1] if '--permission-mode' in sys.argv else 'default'

def emit(value):
    print(json.dumps(value), flush=True)

for line in sys.stdin:
    request = json.loads(line)
    if mode == 'codex':
        method = request.get('method')
        params = request.get('params', {})
        def reply(value):
            emit({'jsonrpc': '2.0', 'id': request['id'], 'result': value})
        if method == 'initialize':
            reply({'serverInfo': {'version': 'policy-fixture/1'}})
        elif method in ('thread/start', 'thread/resume', 'thread/fork'):
            sandbox = json.loads(os.environ.get('SWARMIE_POLICY_OBSERVATION', '{"type":"readOnly","networkAccess":false}'))
            reply({'thread': {'id': 'policy-native'}, 'approvalPolicy': params.get('approvalPolicy', 'on-request'), 'approvalsReviewer': params.get('approvalsReviewer', 'user'), 'sandbox': sandbox, 'model': 'model'})
        elif method == 'model/list':
            reply({'data': [{'id': 'model', 'model': 'model', 'displayName': 'Model'}]})
        elif method == 'turn/start':
            reply({'turn': {'id': 'turn'}})
            emit({'jsonrpc': '2.0', 'method': 'item/agentMessage/delta', 'params': {'threadId': 'policy-native', 'itemId': 'message', 'delta': json.dumps(params)}})
            emit({'jsonrpc': '2.0', 'method': 'turn/completed', 'params': {'threadId': 'policy-native', 'turn': {'id': 'turn', 'status': 'completed'}}})
        elif 'id' in request and method:
            reply({})
    elif mode == 'claude':
        if request.get('type') == 'control_request':
            command = request['request']
            method = command['subtype']
            value = {}
            if method == 'initialize':
                value = {'models': [{'value': 'model', 'displayName': 'Model'}], 'permissionMode': permission, 'version': 'policy-fixture/1'}
            elif method == 'set_permission_mode':
                permission = command['mode']
            emit({'type': 'control_response', 'response': {'subtype': 'success', 'request_id': request['request_id'], 'response': value}})
        elif request.get('type') == 'user':
            session_id = sys.argv[sys.argv.index('--session-id') + 1]
            emit({'type': 'system', 'subtype': 'init', 'session_id': session_id, 'permissionMode': permission})
            emit({'type': 'stream_event', 'event': {'type': 'content_block_delta', 'delta': {'type': 'text_delta', 'text': json.dumps({'permissionMode': permission})}}})
            emit({'type': 'result', 'is_error': False})
