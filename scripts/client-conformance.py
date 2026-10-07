#!/usr/bin/env python3
"""Verify generated clients against a disposable actual runtime daemon."""
import argparse, json, os, pathlib, re, shlex, socket, subprocess, tempfile, time, urllib.request

PROJECT=pathlib.Path(__file__).resolve().parents[1]
CLIENTS=PROJECT/'packages/clients'
ARTIFACTS=pathlib.Path(tempfile.gettempdir())/'mpai-client-conformance'

def run(command,cwd=None,env=None):
    subprocess.run(command,cwd=cwd,env=env,check=True)

def verify_swift(host,port,environment,remote_port):
    directory=subprocess.check_output(['ssh',host,'mktemp -d /tmp/mpai-swift-sdk.XXXXXX'],text=True).strip()
    if not re.fullmatch(r'/tmp/mpai-swift-sdk\.[A-Za-z0-9]+',directory): raise RuntimeError('Unexpected remote temporary directory')
    try:
        run(['rsync','-az','--exclude','.build','--exclude','.swiftpm',str(CLIENTS/'swift')+'/',host+':'+directory+'/'])
        values={key:value for key,value in environment.items() if key.startswith('MPAI_TEST_')}
        values['MPAI_TEST_URL']=f'http://127.0.0.1:{remote_port}'
        assignment=' '.join(key+'='+shlex.quote(value) for key,value in values.items())
        run(['ssh','-o','ExitOnForwardFailure=yes','-R',f'{remote_port}:127.0.0.1:{port}',host,'cd '+shlex.quote(directory)+' && '+assignment+' swift test'])
    finally: run(['ssh',host,'rm -rf '+shlex.quote(directory)])

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary',type=pathlib.Path,default=PROJECT/'target/debug/mpai')
    parser.add_argument('--swift-host',help='Configured SSH alias for a macOS host; required unless --skip-swift is set')
    parser.add_argument('--skip-swift',action='store_true',help='Run Linux clients only; Swift remains explicitly unverified')
    parser.add_argument('--swift-forward-port',type=int,default=17462)
    args=parser.parse_args()
    binary=args.binary.resolve()
    if not binary.is_file(): raise SystemExit('Build the runtime first: cargo build -p app')
    if not args.skip_swift and not args.swift_host: parser.error('--swift-host is required unless --skip-swift is set')
    if args.swift_host and args.swift_host.startswith('-'): raise SystemExit('Invalid SSH host')
    ARTIFACTS.mkdir(parents=True,exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='daemon-',dir=ARTIFACTS) as directory:
        with socket.socket() as listener:listener.bind(('127.0.0.1',0));port=listener.getsockname()[1]
        token='client-conformance-'+os.urandom(16).hex()
        command=[str(binary),'--state-dir',directory,'serve','--http',f'127.0.0.1:{port}','--mock-script',str(PROJECT/'tests/fixtures/client-script.json'),'--token',token]
        daemon=subprocess.Popen(command,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        environment={**os.environ,'MPAI_TEST_URL':f'http://127.0.0.1:{port}','MPAI_TEST_TOKEN':token,'MPAI_TEST_BINARY':str(binary),'MPAI_TEST_SCRIPT':str(PROJECT/'tests/fixtures/client-script.json'),'MPAI_TEST_CWD':str(PROJECT),'MPAI_TOKEN':token}
        try:
            deadline=time.monotonic()+15
            while True:
                if daemon.poll() is not None:raise RuntimeError('Disposable daemon exited before initialization')
                try:
                    request=urllib.request.Request(environment['MPAI_TEST_URL']+'/rpc',json.dumps({'jsonrpc':'2.0','id':1,'method':'runtime/initialize','params':{}}).encode(),{'Authorization':'Bearer '+token,'Content-Type':'application/json'})
                    with urllib.request.urlopen(request,timeout=1) as response:
                        if json.load(response).get('result'):break
                except OSError:
                    if time.monotonic()>deadline:raise RuntimeError('Disposable daemon initialization timed out')
                    time.sleep(.05)
            run(['python3',str(CLIENTS/'tools/generate.py'),'--url',environment['MPAI_TEST_URL'],'--check'],env=environment)
            run(['npm','ci','--ignore-scripts'],cwd=CLIENTS/'typescript',env=environment)
            run(['npm','test'],cwd=CLIENTS/'typescript',env=environment)
            python_env={**environment,'PYTHONPATH':str(CLIENTS/'python')}
            run(['python3','-m','unittest','discover','-s',str(CLIENTS/'python/tests'),'-v'],cwd=PROJECT,env=python_env)
            run(['go','test','-race','./...'],cwd=CLIENTS/'go',env=environment)
            if not args.skip_swift:verify_swift(args.swift_host,port,environment,args.swift_forward_port)
            print('Client conformance passed'+(' (Swift explicitly skipped)' if args.skip_swift else ' (including macOS Swift)'))
        finally:
            daemon.terminate()
            try:daemon.wait(timeout=5)
            except subprocess.TimeoutExpired:daemon.kill();daemon.wait(timeout=5)

if __name__=='__main__':main()
