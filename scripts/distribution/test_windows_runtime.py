#!/usr/bin/env python3
"""Verify Windows installation ownership and archive validation on any test host."""
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import sys
import os
import shutil
import subprocess
import unittest
from unittest.mock import patch

def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(filename))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module
INSTALL = load('windows_install', 'install-runtime.py')
PREPARE = load('windows_prepare', 'prepare-ssh.py')
PACKAGE = load('windows_package', 'package-runtime.py')
ROOT = Path(__file__).resolve().parents[2]

class WindowsInstallation(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.package = self.root / 'package'
        (self.package / 'bin').mkdir(parents=True)
        self.binary = self.package / 'bin/swarmie.exe'
        self.binary.write_bytes(b'native executable fixture')
        self.binary.chmod(0o700)
        for filename in ('LICENSE', 'NOTICE'):
            (self.package / filename).write_bytes((ROOT / filename).read_bytes())
        (self.package / 'runtime-package.json').write_text(json.dumps({'kind': 'swarmie-runtime', 'version': '0.1.0', 'platform': 'windows-x86_64', 'executableSha256': INSTALL.digest(self.binary)}))
        from argparse import Namespace
        self.args = Namespace(prefix=self.root/'user é files/runtime', bin_dir=self.root/'user é files/bin', package=self.package, if_missing=False, uninstall=False)
        for target, attribute, value in [(INSTALL,'WINDOWS',True),(INSTALL,'EXECUTABLE','swarmie.exe'),(INSTALL,'FILES',('bin/swarmie.exe','LICENSE','NOTICE','runtime-package.json'))]:
            context = patch.object(target,attribute,value); context.start(); self.addCleanup(context.stop)
        for name,value in [('system','Windows'),('machine','AMD64')]:
            context=patch.object(INSTALL.platform,name,return_value=value);context.start();self.addCleanup(context.stop)
        context=patch.object(INSTALL,'service_units',return_value=[]);context.start();self.addCleanup(context.stop)

    def test_native_launcher_supports_unicode_and_preserves_user_data_on_removal(self):
        INSTALL.install(self.args)
        launcher=self.args.bin_dir/'swarmie.exe'
        self.assertEqual(launcher.read_bytes(),self.binary.read_bytes())
        self.assertFalse(launcher.is_symlink())
        marker=json.loads((self.args.prefix/INSTALL.MARKER).read_text())
        self.assertEqual(marker['launcherSha256'],hashlib.sha256(launcher.read_bytes()).hexdigest())
        hint = self.args.bin_dir / '.swarmie-runtime-location.json'
        self.assertEqual(json.loads(hint.read_text())['agentToolsDirectory'], str(self.args.prefix / 'agent-tools'))
        user_file=self.args.prefix/'user-document';user_file.write_text('Keep user content')
        self.args.uninstall=True;INSTALL.install(self.args)
        self.assertEqual(user_file.read_text(),'Keep user content')
        self.assertFalse(launcher.exists())
        self.assertFalse(hint.exists())

    def test_unmanaged_or_modified_launcher_support_hint_is_preserved(self):
        self.args.bin_dir.mkdir(parents=True)
        hint = self.args.bin_dir / '.swarmie-runtime-location.json'
        hint.write_text('user-owned hint')
        with self.assertRaisesRegex(SystemExit, 'unmanaged runtime launcher support hint'):
            INSTALL.install(self.args)
        self.assertEqual(hint.read_text(), 'user-owned hint')
        hint.unlink()
        INSTALL.install(self.args)
        hint.write_text('modified hint')
        self.args.uninstall = True
        with self.assertRaisesRegex(SystemExit, 'modified runtime launcher support hint'):
            INSTALL.install(self.args)
        self.assertEqual(hint.read_text(), 'modified hint')

    def test_modified_launcher_and_in_use_executable_refuse_updates(self):
        INSTALL.install(self.args)
        launcher=self.args.bin_dir/'swarmie.exe';original=launcher.read_bytes()
        launcher.write_bytes(b'user modified')
        with self.assertRaisesRegex(SystemExit,'unmanaged launcher'):INSTALL.install(self.args)
        launcher.write_bytes(original)
        replace=INSTALL.os.replace
        def in_use(source,destination):
            if destination == self.args.prefix/'bin/swarmie.exe':raise PermissionError('in use')
            return replace(source,destination)
        with patch.object(INSTALL.os,'replace',in_use),self.assertRaisesRegex(SystemExit,'in use or protected'):
            INSTALL.install(self.args)
        self.assertEqual(launcher.read_bytes(),original)
        self.assertEqual((self.args.prefix/'bin/swarmie.exe').read_bytes(),original)

    def test_windows_probe_and_archive_require_windows_executable(self):
        with patch.object(PREPARE,'EXECUTABLE','swarmie.exe'),patch.object(PREPARE,'FILES',{'bin/swarmie.exe','LICENSE','NOTICE','runtime-package.json','install.py'}):
            self.assertEqual(PREPARE.destination(),'windows-x86_64')
            import tarfile
            archive=self.root/'runtime.tar.gz'
            (self.package/'install.py').write_text('installer fixture')
            with tarfile.open(archive,'w:gz') as bundle:bundle.add(self.package,arcname='runtime')
            destination=self.root/'extracted';destination.mkdir()
            PREPARE.extract(archive,destination)
            self.assertEqual((destination/'bin/swarmie.exe').read_bytes(),self.binary.read_bytes())
            self.assertFalse((destination/'bin/swarmie').exists())

    @unittest.skipUnless(sys.platform == 'win32', 'Requires a real Windows image section')
    def test_loaded_launcher_refuses_update_before_any_managed_file_changes(self):
        image=Path(os.environ['SystemRoot'])/'System32/ping.exe'
        shutil.copy2(image,self.binary)
        metadata=json.loads((self.package/'runtime-package.json').read_text())
        metadata['executableSha256']=INSTALL.digest(self.binary)
        (self.package/'runtime-package.json').write_text(json.dumps(metadata))
        INSTALL.install(self.args)
        marker=self.args.prefix/INSTALL.MARKER
        before=marker.read_bytes()
        saved=json.loads(before)
        process=subprocess.Popen([self.args.bin_dir/'swarmie.exe','-t','127.0.0.1'],stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        try:
            self.assertIsNone(process.poll())
            self.binary.write_bytes(b'Updated package fixture')
            metadata['executableSha256']=INSTALL.digest(self.binary)
            metadata['version']='0.1.1'
            (self.package/'runtime-package.json').write_text(json.dumps(metadata))
            with self.assertRaisesRegex(SystemExit,'in use or protected'):
                INSTALL.install(self.args)
            self.assertIsNone(process.poll())
            self.assertEqual(marker.read_bytes(),before)
            for relative,checksum in saved['files'].items():
                self.assertEqual(INSTALL.digest(self.args.prefix/relative),checksum)
            self.assertEqual(INSTALL.digest(self.args.bin_dir/'swarmie.exe'),saved['launcherSha256'])
        finally:
            process.terminate()
            process.wait(timeout=10)

if __name__ == '__main__':unittest.main()
