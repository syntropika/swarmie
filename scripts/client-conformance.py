#!/usr/bin/env python3
"""Verify generated clients against a disposable actual runtime daemon."""
import argparse, json, os, pathlib, re, shlex, socket, subprocess, tempfile, time, urllib.request

PROJECT=pathlib.Path(__file__).resolve().parents[1]
CLIENTS=PROJECT/'packages/clients'
ARTIFACTS=pathlib.Path(tempfile.gettempdir())/'swarmie-client-conformance'

def run(command,cwd=None,env=None):
    subprocess.run(command,cwd=cwd,env=env,check=True)

def verify_swift(host,port,environment,remote_port):
    directory=subprocess.check_output(['ssh',host,'mktemp -d /tmp/swarmie-swift-sdk.XXXXXX'],text=True).strip()
    if not re.fullmatch(r'/tmp/swarmie-swift-sdk\.[A-Za-z0-9]+',directory): raise RuntimeError('Unexpected remote temporary directory')
    try:
        run(['rsync','-az','--exclude','.build','--exclude','.swiftpm',str(CLIENTS/'swift')+'/',host+':'+directory+'/'])
        values={key:value for key,value in environment.items() if key.startswith('SWARMIE_TEST_')}
        values['SWARMIE_TEST_URL']=f'http://127.0.0.1:{remote_port}'
        assignment=' '.join(key+'='+shlex.quote(value) for key,value in values.items())
        run(['ssh','-o','ExitOnForwardFailure=yes','-R',f'{remote_port}:127.0.0.1:{port}',host,'cd '+shlex.quote(directory)+' && '+assignment+' swift test'])
    finally: run(['ssh',host,'rm -rf '+shlex.quote(directory)])

def verify_typescript_packages():
    import importlib.util
    spec=importlib.util.spec_from_file_location('swarmie_package_local',CLIENTS/'tools/package_local.py')
    packaging=importlib.util.module_from_spec(spec);spec.loader.exec_module(packaging)
    with tempfile.TemporaryDirectory(prefix='typescript-consumer-',dir=ARTIFACTS) as directory:
        root=pathlib.Path(directory);archives=root/'archives';archives.mkdir()
        client,contracts=packaging.package_typescript(archives,build=False)
        consumer=root/'consumer';consumer.mkdir()
        (consumer/'package.json').write_text(json.dumps({'private':True,'type':'module'})+'\n')
        run(['npm','install','--ignore-scripts','--offline','--no-audit','--no-fund',str(contracts),str(client)],cwd=consumer)
        (consumer/'check.ts').write_text('import { Client, type InputSpawnOptions } from "@swarmie/client";\nimport type { SessionProjection } from "@swarmie/contracts";\nconst options: InputSpawnOptions = { harness: "codex", cwd: "/project" };\nconst client = Client.http("http://127.0.0.1:7457");\nvoid client.spawn(options);\nconst projection = null as unknown as SessionProjection;\nvoid projection.events;\n')
        run([str(CLIENTS/'typescript/node_modules/.bin/tsc'),'--strict','--noEmit','--target','ES2022','--module','NodeNext','--moduleResolution','NodeNext','check.ts'],cwd=consumer)
        run(['node','--input-type=module','-e','import { Client } from "@swarmie/client"; if (typeof Client.http !== "function") process.exit(1);'],cwd=consumer)

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary',type=pathlib.Path,default=PROJECT/'target/debug/swarmie')
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
        environment={**os.environ,'SWARMIE_TEST_URL':f'http://127.0.0.1:{port}','SWARMIE_TEST_TOKEN':token,'SWARMIE_TEST_BINARY':str(binary),'SWARMIE_TEST_SCRIPT':str(PROJECT/'tests/fixtures/client-script.json'),'SWARMIE_TEST_CWD':str(PROJECT),'SWARMIE_TOKEN':token}
        try:
            deadline=time.monotonic()+15
            while True:
                if daemon.poll() is not None:raise RuntimeError('Disposable daemon exited before initialization')
                try:
                    request=urllib.request.Request(environment['SWARMIE_TEST_URL']+'/rpc',json.dumps({'jsonrpc':'2.0','id':1,'method':'runtime/initialize','params':{}}).encode(),{'Authorization':'Bearer '+token,'Content-Type':'application/json'})
                    with urllib.request.urlopen(request,timeout=1) as response:
                        if json.load(response).get('result'):break
                except OSError:
                    if time.monotonic()>deadline:raise RuntimeError('Disposable daemon initialization timed out')
                    time.sleep(.05)
            run(['python3',str(CLIENTS/'tools/generate.py'),'--url',environment['SWARMIE_TEST_URL'],'--check'],env=environment)
            run(['npm','ci','--ignore-scripts'],cwd=CLIENTS/'typescript',env=environment)
            run(['npm','test'],cwd=CLIENTS/'typescript',env=environment)
            verify_typescript_packages()
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
