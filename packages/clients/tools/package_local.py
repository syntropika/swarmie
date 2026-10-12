#!/usr/bin/env python3
"""Build local client packages and a current-host runtime executable bundle."""
import argparse, hashlib, json, os, pathlib, platform, shutil, subprocess, sys, tarfile, tempfile

ROOT = pathlib.Path(__file__).resolve().parents[1]
PROJECT = ROOT.parents[1]
DEFAULT_OUTPUT = pathlib.Path(tempfile.gettempdir()) / 'mpai-packages'

def run(command, cwd=None):
    subprocess.run(command, cwd=cwd, check=True)

def copy_source(source, destination):
    shutil.copytree(source,destination,ignore=shutil.ignore_patterns('node_modules','dist','.build','.swiftpm','__pycache__','*.pyc','*.egg-info','build','*.tgz','*.whl'))

def archive(source, output, prefix):
    with tarfile.open(output,'w:gz') as target: target.add(source,arcname=prefix)

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary',type=pathlib.Path,default=PROJECT/'target/release/mpai')
    parser.add_argument('--output',type=pathlib.Path,default=DEFAULT_OUTPUT)
    args=parser.parse_args();binary=args.binary.resolve();output=args.output.resolve()
    if not binary.is_file(): raise SystemExit('Runtime executable is missing. Build with cargo build -p app --release, or supply --binary.')
    if output.is_relative_to(ROOT): raise SystemExit('Package output must be outside the client source directories.')
    version=subprocess.check_output([str(binary),'--version'],text=True,timeout=10).strip()
    package_version=json.loads((ROOT/'typescript/package.json').read_text())['version']
    host_platform={'Linux':'linux','Darwin':'darwin'}.get(platform.system())
    host_arch={'x86_64':'x64','aarch64':'arm64','arm64':'arm64'}.get(platform.machine())
    if not host_platform or not host_arch: raise SystemExit('Local executable bundling supports Linux/macOS x64/arm64 hosts.')
    output.mkdir(parents=True,exist_ok=True)
    run(['npm','ci','--ignore-scripts'],cwd=ROOT/'typescript')
    run(['npm','run','build'],cwd=ROOT/'typescript')
    run(['npm','pack','--pack-destination',str(output)],cwd=ROOT/'typescript')
    with tempfile.TemporaryDirectory(prefix='package-stage-',dir=output) as staging:
        stage=pathlib.Path(staging)
        copy_source(ROOT/'python',stage/'python')
        run([sys.executable,'-m','pip','wheel','--no-deps','--no-build-isolation','--wheel-dir',str(output),str(stage/'python')])
        for language in ['go','swift']:
            copy_source(ROOT/language,stage/language)
            archive(stage/language,output/f'mpai-{language}-client-{package_version}.tar.gz',f'mpai-{language}-client')
        copy_source(ROOT/'runtime',stage/'runtime')
        native=stage/'runtime/native';native.mkdir()
        shutil.copy2(binary,native/'mpai');(native/'mpai').chmod(0o755)
        metadata={'platform':host_platform,'arch':host_arch,'version':version,'build':'debug' if 'debug' in binary.parts else 'release' if 'release' in binary.parts else 'local'}
        (native/'target.json').write_text(json.dumps(metadata,indent=2)+'\n')
        manifest=stage/'runtime/package.json';package=json.loads(manifest.read_text());package.update(os=[host_platform],cpu=[host_arch]);manifest.write_text(json.dumps(package,indent=2)+'\n')
        run(['npm','pack','--pack-destination',str(output)],cwd=stage/'runtime')
        bundle=stage/'native';bundle.mkdir();shutil.copy2(native/'mpai',bundle/'mpai');shutil.copy2(ROOT/'runtime/LICENSE',bundle/'LICENSE');shutil.copy2(native/'target.json',bundle/'target.json')
        archive(bundle,output/f'mpai-{package_version}-{host_platform}-{host_arch}-{metadata["build"]}.tar.gz','mpai')
    shutil.copy2(ROOT/'schema/protocol.json',output/'mpai-protocol-schema.json')
    for artifact in output.iterdir():
        if artifact.suffix in ['.gz','.tgz','.whl']:
            with artifact.open('rb') as stream: digest=hashlib.file_digest(stream,'sha256').hexdigest()
            artifact.with_name(artifact.name+'.sha256').write_text(digest+'  '+artifact.name+'\n')
    print(f'Local distribution artifacts: {output}')

if __name__=='__main__': main()
