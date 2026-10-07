#!/usr/bin/env python3
"""Install or remove a built Linux package in user-owned directories."""
import argparse, json, os, pathlib, shutil, tempfile

MARKER='.mpai-install.json'
def quoted(value):
    return '"'+str(value).replace('\\','\\\\').replace('"','\\"').replace('`','\\`').replace('$','\\$')+'"'
def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--package',type=pathlib.Path,default=pathlib.Path(__file__).resolve().parent)
    parser.add_argument('--prefix',type=pathlib.Path,default=pathlib.Path.home()/'.local/share/mpai/application')
    parser.add_argument('--bin-dir',type=pathlib.Path,default=pathlib.Path.home()/'.local/bin')
    parser.add_argument('--desktop-dir',type=pathlib.Path,default=pathlib.Path.home()/'.local/share/applications')
    parser.add_argument('--uninstall',action='store_true')
    args=parser.parse_args()
    prefix=args.prefix.absolute()
    if any(c in str(prefix) for c in '\n\r\x00%'): raise SystemExit('Installation path contains unsupported desktop-entry characters')
    if prefix.is_symlink(): raise SystemExit('Installation prefix must not be a symbolic link')
    marker=prefix/MARKER
    if prefix.exists() and not marker.is_file(): raise SystemExit('Refusing to replace an unmanaged installation directory')
    links={args.bin_dir.absolute()/'mpai':prefix/'bin/mpai-desktop_services',args.bin_dir.absolute()/'mpai-desktop':prefix/'bin/mpai-desktop'}
    desktop=args.desktop_dir.absolute()/'dev.mpai.desktop.desktop'
    if args.uninstall:
        if not marker.is_file(): raise SystemExit('No managed installation exists at this prefix')
        saved=json.loads(marker.read_text())
        for path,target in saved['links'].items():
            path=pathlib.Path(path)
            if path.is_symlink() and os.readlink(path)==target:path.unlink()
        saved_desktop=pathlib.Path(saved['desktop'])
        if saved_desktop.is_file() and f'X-Mpai-Install={prefix}' in saved_desktop.read_text():saved_desktop.unlink()
        shutil.rmtree(prefix)
        print('Removed application files; session data and user services are preserved.')
        return
    package=args.package.resolve()
    for path in ('bin/mpai-desktop','bin/mpai-desktop_services','resources/index.html'):
        if not (package/path).is_file(): raise SystemExit('Incomplete package: '+path)
    for path,target in links.items():
        if path.exists() or path.is_symlink():
            if not path.is_symlink() or os.readlink(path)!=str(target):raise SystemExit('Refusing to replace unmanaged launcher: '+str(path))
    if desktop.exists() and f'X-Mpai-Install={prefix}' not in desktop.read_text():raise SystemExit('Refusing to replace unmanaged desktop entry')
    prefix.parent.mkdir(parents=True,exist_ok=True)
    staging=pathlib.Path(tempfile.mkdtemp(prefix='.mpai-install-',dir=prefix.parent))
    previous=prefix.with_name(prefix.name+'.previous')
    try:
        shutil.copytree(package,staging,dirs_exist_ok=True,symlinks=False)
        (staging/MARKER).write_text(json.dumps({'links':{str(p):str(t) for p,t in links.items()},'desktop':str(desktop)},indent=2)+'\n')
        if previous.exists():raise SystemExit('Previous installation backup exists: '+str(previous))
        if prefix.exists():prefix.rename(previous)
        staging.rename(prefix)
        for path,target in links.items():
            path.parent.mkdir(parents=True,exist_ok=True)
            if path.is_symlink():path.unlink()
            path.symlink_to(target)
        desktop.parent.mkdir(parents=True,exist_ok=True)
        icon=prefix/'share/icons/icon.png'
        desktop.write_text('[Desktop Entry]\nType=Application\nName=mpai\nComment=Agent workspace\nExec='+quoted(prefix/'bin/mpai-desktop')+'\nIcon='+str(icon)+'\nTerminal=false\nCategories=Development;\nX-Mpai-Install='+str(prefix)+'\n')
        if previous.exists():shutil.rmtree(previous)
    finally:
        if staging.exists():shutil.rmtree(staging)
    print('Installed '+str(prefix)+'; no daemon service was activated.')
if __name__=='__main__':main()
