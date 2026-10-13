#!/usr/bin/env python3
"""Verify native package inputs include licenses before resource sealing."""
import importlib.util
import json
import sys
import zipfile
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

SCRIPT = Path(__file__).with_name('package.py')
SPEC = importlib.util.spec_from_file_location('swarmie_package', SCRIPT)
PACKAGE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PACKAGE)


class PackageTests(unittest.TestCase):
    def test_macos_licenses_are_inputs_to_signing_and_notarization(self):
        with tempfile.TemporaryDirectory() as temporary:
            assets = Path(temporary) / 'frontend'
            assets.mkdir()
            (assets / 'index.html').write_text('frontend fixture')
            command = ['native', 'package', '--assets', assets, '--signing', 'identity',
                       '--identity', 'fixture identity', '--notarize', '--notary-profile', 'fixture profile']
            seen = []

            def native_packaging(arguments, cwd):
                staged = arguments[arguments.index('--assets') + 1]
                self.assertNotEqual(staged, assets)
                self.assertEqual((staged / 'index.html').read_text(), 'frontend fixture')
                self.assertIn('Apache License', (staged / 'licenses/swarmie/LICENSE').read_text())
                self.assertIn('Copyright 2026 Syntropika', (staged / 'licenses/swarmie/NOTICE').read_text())
                self.assertIn('--notarize', arguments)
                seen.append(staged)

            with patch.object(PACKAGE, 'run', native_packaging):
                PACKAGE.package_native(command, Path(temporary), assets, 'macos')
            self.assertEqual(len(seen), 1)
            self.assertFalse(seen[0].exists())
            self.assertFalse((assets / 'licenses').exists())
            self.assertEqual(command[command.index('--assets') + 1], assets)

    def test_windows_zip_contains_native_binaries_loader_and_licenses_without_generated_docs(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            desktop = root / 'apps/desktop'
            for directory in ('zig-out/bin', 'dist', 'node_modules/@native-sdk/cli/third_party/webview2'):
                (desktop / directory).mkdir(parents=True)
            (root / 'target/release').mkdir(parents=True)
            (root / 'target/release/swarmie.exe').write_bytes(b'runtime fixture')
            (desktop / 'zig-out/bin/swarmie-desktop.exe').write_bytes(b'host fixture')
            (desktop / 'dist/index.html').write_text('frontend fixture')
            (desktop / 'app.json').write_text(json.dumps({'version': '1.2.3'}))
            (desktop / 'node_modules/@native-sdk/cli/third_party/webview2/LICENSE.txt').write_text('Loader license fixture')
            for name in ('LICENSE', 'NOTICE'):
                (root / name).write_bytes((PACKAGE.PROJECT / name).read_bytes())
            output = root / 'output'
            def native_package(command, cwd):
                self.assertEqual(command[command.index('--target') + 1], 'windows')
                bundle = Path(command[command.index('--output') + 1])
                (bundle / 'bin').mkdir(parents=True)
                (bundle / 'bin/swarmie-desktop.exe').write_bytes(b'host fixture')
                (bundle / 'bin/swarmie-desktop_services.exe').write_bytes(b'runtime fixture')
                (bundle / 'bin/WebView2Loader.dll').write_bytes(b'loader fixture')
                (bundle / 'README.txt').write_text('Excluded generated usage document')
            with patch.object(PACKAGE, 'PROJECT', root), patch.object(PACKAGE.platform, 'system', return_value='Windows'), patch.object(PACKAGE.platform, 'machine', return_value='AMD64'), patch.object(PACKAGE, 'run', native_package), patch.object(PACKAGE.subprocess, 'check_output', return_value='swarmie 1.2.3'), patch.object(sys, 'argv', ['package.py', '--skip-build', '--output', str(output), '--release-base-url', 'https://releases.example.com/v1.2.3']):
                PACKAGE.main()
            archive = output / 'swarmie-1.2.3-windows-x86_64.zip'
            prefix = archive.stem + '/'
            with zipfile.ZipFile(archive) as package:
                self.assertIn(prefix + 'bin/swarmie-desktop_services.exe', package.namelist())
                self.assertIn(prefix + 'bin/WebView2Loader.dll', package.namelist())
                self.assertIn(prefix + 'share/licenses/swarmie/NOTICE', package.namelist())
                self.assertIn(prefix + 'share/licenses/webview2/LICENSE.txt', package.namelist())
                self.assertNotIn(prefix + 'README.txt', package.namelist())
            manifest = json.loads((output / 'manifest.json').read_text())
            self.assertEqual(set(manifest['artifacts']), {'windows-x86_64'})


if __name__ == '__main__':
    unittest.main()
