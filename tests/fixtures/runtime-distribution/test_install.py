#!/usr/bin/env python3
"""Qualify runtime-only installation ownership and preservation in disposable directories."""
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import tempfile
import unittest

PROJECT = Path(__file__).resolve().parents[3]
INSTALLER = PROJECT / 'scripts/distribution/install-runtime.py'


class RuntimeInstallerTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.package = self.root / 'package'
        self.prefix = self.root / 'runtime'
        self.bin_dir = self.root / 'bin'
        self.home = self.root / 'home'
        self.home.mkdir()
        self.env = dict(os.environ, HOME=str(self.home), XDG_CONFIG_HOME=str(self.home / '.config'))
        (self.package / 'bin').mkdir(parents=True)
        for name in ('LICENSE', 'NOTICE'):
            (self.package / name).write_bytes((PROJECT / name).read_bytes())
        self.write_runtime(b'#!/bin/sh\nprintf "fixture runtime\\n"\n')

    def write_runtime(self, content):
        binary = self.package / 'bin/swarmie'
        binary.write_bytes(content)
        binary.chmod(0o755)
        arch = {'x86_64': 'x86_64', 'amd64': 'x86_64', 'aarch64': 'aarch64', 'arm64': 'aarch64'}[platform.machine()]
        system = {'Linux': 'linux', 'Darwin': 'macos'}[platform.system()]
        (self.package / 'runtime-package.json').write_text(json.dumps({
            'kind': 'swarmie-runtime', 'version': '0.1.0', 'platform': system + '-' + arch,
            'executableSha256': hashlib.sha256(content).hexdigest()}))

    def run_installer(self, *args, success=True):
        result = subprocess.run(['python3', str(INSTALLER), '--package', str(self.package),
                                 '--prefix', str(self.prefix), '--bin-dir', str(self.bin_dir), *args],
                                env=self.env, text=True, capture_output=True)
        self.assertEqual(result.returncode == 0, success, result.stderr)
        return result

    def test_update_and_uninstall_preserve_user_files_even_inside_prefix(self):
        self.run_installer()
        self.assertEqual((self.bin_dir / 'swarmie').resolve(), (self.prefix / 'bin/swarmie').resolve())
        history = self.prefix / 'user-sessions'
        history.mkdir()
        (history / 'retained.json').write_text('owned user data')
        project = self.root / 'project'
        project.mkdir()
        (project / 'work.txt').write_text('owned working directory')
        self.write_runtime(b'#!/bin/sh\nprintf "updated runtime\\n"\n')
        updated = json.loads(self.run_installer().stdout)
        self.assertTrue(updated['restartRequiredForRunningProcesses'])
        self.assertFalse(updated['processesStopped'])
        self.assertIn('updated runtime', (self.prefix / 'bin/swarmie').read_text())
        self.run_installer('--uninstall')
        self.assertEqual((history / 'retained.json').read_text(), 'owned user data')
        self.assertEqual((project / 'work.txt').read_text(), 'owned working directory')
        self.assertFalse((self.bin_dir / 'swarmie').is_symlink())
        self.assertFalse((self.prefix / 'bin/swarmie').exists())

    def test_modified_owned_files_are_never_replaced_or_deleted(self):
        self.run_installer()
        (self.prefix / 'NOTICE').write_text('user modified file')
        self.assertIn('modified installation file', self.run_installer(success=False).stderr)
        self.run_installer('--uninstall', success=False)
        self.assertEqual((self.prefix / 'NOTICE').read_text(), 'user modified file')

    def test_unmanaged_directory_and_launcher_are_preserved(self):
        self.prefix.mkdir()
        (self.prefix / 'file').write_text('unmanaged')
        self.run_installer(success=False)
        (self.prefix / 'file').unlink()
        self.bin_dir.mkdir()
        (self.bin_dir / 'swarmie').write_text('user launcher')
        self.run_installer(success=False)
        self.assertEqual((self.bin_dir / 'swarmie').read_text(), 'user launcher')

    def test_checksum_and_platform_are_checked_before_installation(self):
        (self.package / 'bin/swarmie').write_bytes(b'changed')
        self.assertIn('SHA-256', self.run_installer(success=False).stderr)
        self.assertFalse(self.prefix.exists())
        self.write_runtime(b'#!/bin/sh\nexit 0\n')
        metadata = self.package / 'runtime-package.json'
        content = json.loads(metadata.read_text())
        content['platform'] = 'other-platform'
        metadata.write_text(json.dumps(content))
        self.assertIn('host platform', self.run_installer(success=False).stderr)

    def test_installed_service_must_be_removed_explicitly(self):
        self.run_installer()
        unit = self.home / '.config/systemd/user/swarmie.service'
        unit.parent.mkdir(parents=True)
        unit.write_text('ExecStart="' + str(self.prefix / 'bin/swarmie') + '" serve\n')
        self.assertIn('service still references', self.run_installer('--uninstall', success=False).stderr)
        self.assertTrue((self.prefix / 'bin/swarmie').is_file())
        unit.unlink()
        self.run_installer('--uninstall')

    def test_symlinked_prefix_and_owned_binary_are_refused(self):
        target = self.root / 'other'
        target.mkdir()
        self.prefix.symlink_to(target)
        self.run_installer(success=False)
        self.prefix.unlink()
        self.run_installer()
        (self.prefix / 'bin/swarmie').unlink()
        (self.prefix / 'bin/swarmie').symlink_to(self.package / 'bin/swarmie')
        self.run_installer('--uninstall', success=False)
        self.assertTrue((self.package / 'bin/swarmie').is_file())


if __name__ == '__main__':
    unittest.main()
