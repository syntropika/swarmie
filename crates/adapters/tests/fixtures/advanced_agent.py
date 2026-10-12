#!/usr/bin/env python3
import sys, os, json, queue, threading, time, pathlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

mode = sys.argv[1]
session = 'original-session'
action = None
mcp = []
def send(value):
    print(json.dumps(value), flush=True)
def receipt():
    return {'env': os.environ.get('MPAI_TEST_ENV'), 'piHome': os.environ.get('PI_CODING_AGENT_DIR'),
            'openCodeHome': os.environ.get('OPENCODE_CONFIG_DIR'), 'args': sys.argv,
            'inline': json.loads(os.environ.get('OPENCODE_CONFIG_CONTENT', '{}')), 'mcp': mcp, 'action': action}

if mode == 'serve':
    events = queue.Queue()
    requests = []
    history_file = pathlib.Path.cwd() / 'opencode-sessions.json'
    initial_history = [
        {'info':{'id':'user-1','role':'user','model':{'providerID':'fixture','modelID':'model'}}},
        {'info':{'id':'message-9','role':'assistant','finish':'stop','time':{'completed':123}}},
        {'info':{'id':'user-2','role':'user','model':{'providerID':'fixture','modelID':'model'}}},
        {'info':{'id':'message-10','role':'assistant','time':{'completed':124}}},
    ]
    histories = json.loads(history_file.read_text()) if history_file.exists() else {session: initial_history}
    def emit(kind, **props):
        events.put({'type': kind, 'properties': dict(sessionID=session, **props)})
    class Handler(BaseHTTPRequestHandler):
        protocol_version = 'HTTP/1.1'
        def log_message(self, *_): pass
        def reply(self, value):
            data = json.dumps(value).encode()
            self.send_response(200)
            self.send_header('Content-Length', str(len(data)))
            self.send_header('Content-Type', 'application/json')
            self.end_headers()
            self.wfile.write(data)
        def do_GET(self):
            path = self.path.split('?',1)[0]
            if path == '/event':
                self.send_response(200)
                self.send_header('Content-Type','text/event-stream')
                self.end_headers()
                self.wfile.write(b': ready\n\n'); self.wfile.flush()
                try:
                    while True:
                        try: data = 'data: '+json.dumps(events.get(timeout=.1))+'\n\n'
                        except queue.Empty: data = ': alive\n\n'
                        self.wfile.write(data.encode()); self.wfile.flush()
                except (BrokenPipeError,ConnectionResetError): pass
            elif path == '/global/health': self.reply({'healthy':True})
            elif path == '/config/providers':
                self.reply({'providers':[{'id':'fixture','models':{
                    'model':{'name':'Fixture model','limit':{'context':2000},'variants':{'low':{},'high':{},'disabled':{'disabled':True}}},
                    'other':{'name':'Other model','limit':{'context':4000},'variants':{'high':{},'max':{}}},
                    'plain':{'name':'Plain model','limit':{'context':1000}},
                }}]})
            elif path == '/command': self.reply([{'name':'review','description':'Review a change'}])
            elif path == '/session/status': self.reply({})
            elif path.endswith('/message'):
                self.reply(histories.get(path.split('/')[2], initial_history))
            else: self.reply({'id':path.split('/')[2] if path.startswith('/session/') else session})
        def do_POST(self):
            global session
            path = self.path.split('?',1)[0]
            body = json.loads(self.rfile.read(int(self.headers.get('Content-Length','0'))) or '{}')
            requests.append({'path':path,'body':body})
            if path == '/session': self.reply({'id':session})
            elif path.endswith('/fork'):
                source = histories.get(path.split('/')[2], initial_history)
                boundary = body.get('messageID')
                end = next((index for index, message in enumerate(source) if message['info']['id'] == boundary), len(source))
                session = 'fork:'+body.get('messageID','latest')
                cloned = json.loads(json.dumps(source[:end]))
                for index, message in enumerate(cloned): message['info']['id'] = f'{session}:cloned-{index}'
                if os.environ.get('MPAI_TEST_BAD_FORK') and cloned: cloned.pop()
                histories[session] = cloned
                history_file.write_text(json.dumps(histories))
                self.reply({'id':session})
            elif path.endswith('/prompt_async'):
                self.reply(None)
                if body.get('parts',[{}])[0].get('text') == 'permission-request':
                    emit('permission.asked',id='per-test',permission='bash',title='Run a command',patterns=['echo receipt'])
                    return
                result = receipt(); result['request'] = body; result['requests'] = requests
                emit('message.updated',info={'sessionID':session,'id':'assistant','role':'assistant','providerID':'fixture','modelID':'model'})
                emit('message.part.updated',part={'sessionID':session,'messageID':'assistant','id':'task-part','type':'tool','tool':'task','state':{'status':'completed','title':'Inspect project','input':{'description':'Inspect project'},'output':'done','metadata':{'sessionId':'child-7','parentSessionId':session,'model':'fixture/model'}}})
                emit('message.part.delta',messageID='assistant',partID='text',field='text',delta=json.dumps(result))
                emit('message.updated',info={'sessionID':session,'id':'assistant','role':'assistant','providerID':'fixture','modelID':'model',
                     'finish':'stop','time':{'completed':123},'tokens':{'input':100,'output':20,'cache':{'read':30,'write':10}},'structured':{'ok':True}})
                emit('session.idle')
            elif path == '/permission/per-test/reply':
                self.reply(True)
                emit('message.part.delta',messageID='assistant',partID='text',field='text',delta=json.dumps({'permissionReply':body}))
                emit('session.idle')
            else: self.reply(True)
    port = int(sys.argv[sys.argv.index('--port')+1])
    ThreadingHTTPServer(('127.0.0.1',port),Handler).serve_forever()
else:
    for line in sys.stdin:
        r = json.loads(line)
        method = r.get('type') if mode == 'pi' else r.get('method')
        if mode == 'pi':
            data = {}
            if method == 'get_state': data = {'sessionId':session,'model':{'id':'model','provider':'fixture','contextWindow':1000}}
            elif method in ('fork','clone'):
                action = r; session = 'fork:'+r['entryId'] if method == 'fork' else 'cloned-session'
                data = {'cancelled':False}
            elif method == 'get_session_stats': data = {'contextUsage':{'tokens':240,'contextWindow':1000,'percent':24}}
            elif method == 'get_commands': data = {'commands':[{'name':'skill:review','source':'skill'}]}
            elif method == 'get_available_models': data = {'models':[{'id':'model','provider':'fixture','name':'Fixture'}]}
            elif method == 'prompt': data = {'disposition':'started'}
            send({'type':'response','id':r.get('id'),'success':True,'data':data,'command':method})
            if method == 'prompt':
                send({'type':'message_update','assistantMessageEvent':{'type':'text_delta','delta':json.dumps(receipt())}})
                send({'type':'message_end','message':{'role':'assistant','stopReason':'stop','usage':{'totalTokens':99999}}})
                send({'type':'agent_settled'})
        else:
            data = {}
            if method == 'initialize':
                data = {'protocolVersion':1,'agentCapabilities':{'loadSession':True,'mcpCapabilities':{'http':mode=='acp-http','sse':False}}}
            elif method in ('session/new','session/load'):
                mcp = r['params']['mcpServers']
                data = {'sessionId':session,'models':{'currentModelId':'model','availableModels':[{'modelId':'model','name':'Fixture'}]}}
            elif method == 'session/prompt': data = {'stopReason':'end_turn'}
            if method == 'session/prompt':
                for update in [
                    {'sessionUpdate':'available_commands_update','availableCommands':[{'name':'review','description':'Review'}]},
                    {'sessionUpdate':'tool_call','toolCallId':'call-1','title':'Read file','kind':'read','rawInput':{'path':'hello.txt'},'_meta':{'mcp':{'server':'fixture','tool':'read'}}},
                    {'sessionUpdate':'tool_call_update','toolCallId':'call-1','status':'completed','rawOutput':'','content':[{'type':'content','content':{'type':'text','text':'contents'}}]},
                    {'sessionUpdate':'agent_message_chunk','content':{'type':'text','text':''},'_meta':{'usage':{'inputTokens':100,'outputTokens':30,'cachedReadTokens':20,'thoughtTokens':5,'totalTokens':130}}},
                    {'sessionUpdate':'usage_update','used':400,'size':2000},
                    {'sessionUpdate':'agent_message_chunk','content':{'type':'text','text':json.dumps(receipt())}}]:
                    send({'jsonrpc':'2.0','method':'session/update','params':{'sessionId':session,'update':update}})
            if 'id' in r: send({'jsonrpc':'2.0','id':r['id'],'result':data})
