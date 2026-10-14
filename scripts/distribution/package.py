#!/usr/bin/env python3
"""Build a native host, runtime, frontend and verifiable release archive on the current platform."""
import argparse, hashlib, json, pathlib, platform, shutil, subprocess, tarfile, tempfile, urllib.parse, zipfile
PROJECT=pathlib.Path(__file__).resolve().parents[2]
def run(command,cwd=PROJECT):subprocess.run([str(x) for x in command],cwd=cwd,check=True)
def package_native(command, desktop, assets, system):
    if system != 'macos':
        run(command, desktop)
        return
    # Native packaging signs and notarizes the final resource tree. Add licenses
    # to its input assets so nothing modifies the sealed bundle afterward.
    with tempfile.TemporaryDirectory(prefix='swarmie-package-assets-') as temporary:
        staged = pathlib.Path(temporary)/'assets'
        shutil.copytree(assets, staged)
        licenses = staged/'licenses/swarmie'
        licenses.mkdir(parents=True, exist_ok=True)
        for filename in ('LICENSE', 'NOTICE'):
            shutil.copy2(PROJECT/filename, licenses/filename)
        command = list(command)
        command[command.index('--assets')+1] = staged
        run(command, desktop)

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',type=pathlib.Path,default=pathlib.Path(tempfile.gettempdir())/'swarmie-distribution')
    parser.add_argument('--skip-build',action='store_true',help='Require existing release binaries and frontend assets')
    parser.add_argument('--signing',choices=['none','adhoc','identity'],default='adhoc')
    parser.add_argument('--identity')
    parser.add_argument('--notary-profile')
    parser.add_argument('--release-base-url',help='HTTPS directory for a generated updater manifest')
    args=parser.parse_args()
    system={'Darwin':'macos','Linux':'linux','Windows':'windows'}.get(platform.system())
    if system is None:raise SystemExit('Packaging supports Linux, macOS and Windows.')
    architecture={'arm64':'aarch64','aarch64':'aarch64','x86_64':'x86_64','amd64':'x86_64'}.get(platform.machine().lower())
    if architecture is None:raise SystemExit('Unsupported package architecture')
    if args.signing=='identity' and not args.identity:raise SystemExit('--identity is required for identity signing')
    if args.notary_profile and args.signing!='identity':raise SystemExit('Notarization requires identity signing')
    if args.release_base_url:
        url=urllib.parse.urlsplit(args.release_base_url)
        if url.scheme!='https' or not url.netloc or url.username or url.password or url.query or url.fragment:raise SystemExit('Release base URL must be an HTTPS directory without credentials, query or fragment')
    desktop=PROJECT/'apps/desktop'
    metadata=json.loads((desktop/'app.json').read_text())
    version=metadata['version']
    output=args.output.expanduser().resolve();output.mkdir(parents=True,exist_ok=True)
    if not args.skip_build:
        run(['cargo','build','--locked','--release','-p','app'])
        run(['bun','install','--frozen-lockfile'])
        run(['bun','run','--cwd','apps/desktop','build:web'])
        run(['npm','exec','--','native','build'],desktop)
    suffix='.exe' if system=='windows' else ''
    daemon=PROJECT/('target/release/swarmie'+suffix);host=desktop/('zig-out/bin/swarmie-desktop'+suffix);assets=desktop/'dist'
    if not all(p.is_file() for p in (daemon,host,assets/'index.html')):raise SystemExit('Build release runtime, native host and frontend first')
    if subprocess.check_output([str(daemon),'--version'],text=True).strip()!='swarmie '+version:raise SystemExit('Runtime and app manifest versions differ; rebuild matching release inputs')
    name=f'swarmie-{version}-{system}-{architecture}'
    bundle=output/(name+'.app' if system=='macos' else name)
    if bundle.exists():raise SystemExit('Output already exists: '+str(bundle))
    command=['npm','exec','--','native','package','--target',system,'--output',bundle,'--binary',host,'--service-binary',daemon,'--assets',assets,'--web-engine','system','--web-layer','include']
    if system=='macos':
        command += ['--signing',args.signing]
        if args.identity:command += ['--identity',args.identity]
        if args.notary_profile:command += ['--notarize','--notary-profile',args.notary_profile]
    package_native(command,desktop,assets,system)
    if system in ('linux','windows'):
        # Project documentation is kept separately from public distribution packages.
        generated_readme=bundle/'README.txt'
        if generated_readme.is_file():generated_readme.unlink()
    licenses=bundle/('Contents/Resources/licenses/swarmie' if system=='macos' else 'share/licenses/swarmie')
    if system in ('linux','windows'):
        licenses.mkdir(parents=True,exist_ok=True)
        for filename in ('LICENSE','NOTICE'):shutil.copy2(PROJECT/filename,licenses/filename)
    elif not all((licenses/filename).is_file() for filename in ('LICENSE','NOTICE')):
        raise SystemExit('Native package did not include the licenses before signing')
    if system=='linux':
        shutil.copy2(PROJECT/'scripts/distribution/install-linux.py',bundle/'install.py')
        icons=bundle/'share/icons';icons.mkdir(parents=True,exist_ok=True)
        shutil.copy2(desktop/'assets/icon.png',icons/'icon.png')
        archive=output/(name+'.tar.gz')
        with tarfile.open(archive,'w:gz') as result:result.add(bundle,arcname=name)
    elif system=='windows':
        if not (bundle/'bin/WebView2Loader.dll').is_file():raise SystemExit('Windows package requires the architecture-matched WebView2 loader.')
        loader_license=desktop/'node_modules/@native-sdk/cli/third_party/webview2/LICENSE.txt'
        if not loader_license.is_file():raise SystemExit('Windows package requires the WebView2 loader license.')
        loader_licenses=bundle/'share/licenses/webview2';loader_licenses.mkdir(parents=True,exist_ok=True)
        shutil.copy2(loader_license,loader_licenses/'LICENSE.txt')
        archive=output/(name+'.zip')
        with zipfile.ZipFile(archive,'w',zipfile.ZIP_DEFLATED) as result:
            for path in sorted(bundle.rglob('*')):
                if path.is_file():result.write(path,path.relative_to(output))
    else:
        run(['codesign','--verify','--deep','--strict','--verbose=2',bundle]) if args.signing!='none' else None
        archive=output/(name+'.zip')
        run(['/usr/bin/ditto','-c','-k','--sequesterRsrc','--keepParent',bundle,archive])
    hasher=hashlib.sha256()
    with archive.open('rb') as file:
        for chunk in iter(lambda:file.read(1024*1024),b''):hasher.update(chunk)
    digest=hasher.hexdigest()
    (output/(archive.name+'.sha256')).write_text(digest+'  '+archive.name+'\n')
    receipt={'version':version,'platform':system+'-'+architecture,'archive':str(archive),'sha256':digest,'signing':args.signing if system=='macos' else 'none','notarized':bool(args.notary_profile)}
    if args.release_base_url:
        artifact={'url':args.release_base_url.rstrip('/')+'/'+urllib.parse.quote(archive.name),'sha256':digest,'fileName':archive.name}
        manifest={'version':version,'artifacts':{receipt['platform']:artifact}}
        (output/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
    print(json.dumps(receipt,indent=2))
if __name__=='__main__':main()
