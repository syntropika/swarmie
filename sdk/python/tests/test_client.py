import json, os, pathlib, tempfile, threading, time, unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from swarmie_client import Client, HttpTransport, StdioTransport, RpcError, ProtocolError
from swarmie_client.client import decode_response

class Handler(BaseHTTPRequestHandler):
    def log_message(self,*args): pass
    def do_POST(self):
        request=json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        if request['method']=='slow': time.sleep(.1)
        self.send_response(200);self.end_headers()
        try:self.wfile.write(json.dumps({'jsonrpc':'2.0','id':'wrong' if request['method']=='wrong' else request['id'],'result':([{'sessionId':'unordered','sequence':2},{'sessionId':'unordered','sequence':1}] if request.get('params',{}).get('sessionId')=='unordered' else [] if request['method']=='session/transcript' else 'x'*2048 if request['method']=='large' else None)}).encode())
        except BrokenPipeError:pass

class Clients(unittest.TestCase):
    def test_identity_and_structured_errors(self):
        self.assertIsNone(decode_response({'jsonrpc':'2.0','id':'a','result':None},'a'))
        with self.assertRaises(ProtocolError):decode_response({'jsonrpc':'2.0','id':'b','result':None},'a')
        with self.assertRaises(RpcError) as caught:decode_response({'jsonrpc':'2.0','id':'a','error':{'code':-32602,'message':'invalid','data':{'field':'cwd'}}},'a')
        self.assertEqual(caught.exception.code,-32602);self.assertEqual(caught.exception.data,{'field':'cwd'})
    def test_http_bounds(self):
        server=ThreadingHTTPServer(('127.0.0.1',0),Handler);threading.Thread(target=server.serve_forever,daemon=True).start()
        client=Client.http(f'http://127.0.0.1:{server.server_port}',max_response_bytes=1024)
        try:
            self.assertIsNone(client.call('null'))
            with self.assertRaises(ProtocolError):client.call('wrong')
            with self.assertRaises(ProtocolError):client.call('large')
        finally:server.shutdown();server.server_close()
    def test_polling_deadline_and_order(self):
        server=ThreadingHTTPServer(('127.0.0.1',0),Handler);threading.Thread(target=server.serve_forever,daemon=True).start()
        with Client.http(f'http://127.0.0.1:{server.server_port}') as client:
            try:
                with self.assertRaises(TimeoutError):client.wait_for_turn('s',timeout=.02,poll_interval=.002)
                with self.assertRaises(ProtocolError):list(client.events('unordered'))
            finally:server.shutdown();server.server_close()
    def test_cancellation_interrupts_active_native_io(self):
        server=ThreadingHTTPServer(('127.0.0.1',0),Handler);threading.Thread(target=server.serve_forever,daemon=True).start()
        with Client.http(f'http://127.0.0.1:{server.server_port}') as client:
            cancel=threading.Event()
            timer=threading.Timer(.01,cancel.set)
            try:
                timer.start()
                with self.assertRaises(InterruptedError):client.call('slow',cancel=cancel)
                with self.assertRaises(InterruptedError):client.call('slow',cancel=cancel)
            finally:timer.cancel();server.shutdown();server.server_close()
    def test_owned_stdio_timeout(self):
        import sys
        transport=StdioTransport([sys.executable,'-c','import time;time.sleep(60)'],timeout=.02)
        try:
            with self.assertRaises(TimeoutError):Client(transport).initialize()
            with self.assertRaises(ProtocolError):Client(transport).initialize()
        finally:transport.close()
    def conformance(self,client):
        self.assertTrue(client.initialize());self.assertIn('session/create',client.schema()['x-methods'])
        self.assertTrue(any(h['harness']=='mock' and h['available'] for h in client.discover()))
        with self.assertRaises(RpcError) as caught:client.call('not/a/method')
        self.assertEqual(caught.exception.code,-32601)
        session=client.spawn({'harness':'mock','cwd':os.environ.get('SWARMIE_TEST_CWD',os.getcwd())})
        try:
            self.assertEqual(session['capabilities']['queue']['source'],'runtime')
            self.assertEqual(session['capabilities']['configure']['availability'],'available')
            models=client.call('agent/models',{'sessionId':session['id']})
            self.assertEqual(models[0]['metadata']['nativeField'],'retained')
            settings=client.call('agent/configuration',{'sessionId':session['id']})
            self.assertEqual(settings[0]['application'],'next_turn')
            configured=client.call('agent/configure',{'sessionId':session['id'],'config':{'fast':True}})
            self.assertTrue(configured['values']['fast'])
            self.assertTrue(configured['configuration'][0]['value'])
            self.assertEqual(settings[-1]['id'],'executionProfile')
            self.assertEqual(configured['executionPolicy']['status'],'unknown')
            self.assertFalse(configured['executionPolicy']['dispatchBlocked'])
            self.assertEqual(len(configured['executionPolicy']['choices']),3)
            self.assertEqual(client.prompt(session['id'],'Reply','request-1')['delivery'], 'accepted')
            events=client.wait_for_turn(session['id'],after_sequence=session['sequence'],timeout=3,poll_interval=.005)
            self.assertEqual(''.join(e['data']['text'] for e in events if e['type']=='text.delta'),'client-ready')
            self.assertEqual(events[-1]['type'],'turn.ended')
            self.assertEqual(events[-1]['data']['outcome'],'completed')
            self.assertEqual(sorted(e['sequence'] for e in events),[e['sequence'] for e in events])
            recovered = client.submission(session['id'], 'request-1')
            self.assertEqual(client.reconcile_submission(session['id'], 'request-1')['id'], recovered['id'])
            self.assertEqual(recovered['outcome'], 'completed')
            self.assertEqual(client.prompt(session['id'], 'Reply', 'request-1')['id'], recovered['id'])
            with self.assertRaises(RpcError) as conflict: client.prompt(session['id'], 'Changed', 'request-1')
            self.assertEqual(conflict.exception.code, -32004)
            self.assertEqual(conflict.exception.data['code'], 'idempotency_conflict')
            self.assertTrue(any(e['type']=='text.delta' for e in client.call('session/transcript',{'sessionId':session['id'],'limit':4096})))
        finally:client.close(session['id'])
    @unittest.skipUnless(os.environ.get('SWARMIE_TEST_URL'),'live daemon URL required')
    def test_live_http(self):
        self.conformance(Client.http(os.environ['SWARMIE_TEST_URL'],token=os.environ.get('SWARMIE_TEST_TOKEN')))
        with self.assertRaises(ProtocolError):Client.http(os.environ['SWARMIE_TEST_URL'],token='incorrect').initialize()
    @unittest.skipUnless(os.environ.get('SWARMIE_TEST_BINARY'),'daemon binary required')
    def test_live_stdio(self):
        with tempfile.TemporaryDirectory(prefix='swarmie-python-client-') as state:
            transport=StdioTransport([os.environ['SWARMIE_TEST_BINARY'],'--state-dir',state,'serve','--mock-script',os.environ['SWARMIE_TEST_SCRIPT']])
            try:self.conformance(Client(transport))
            finally:transport.close()

if __name__=='__main__':unittest.main()
