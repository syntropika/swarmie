#!/usr/bin/env python3
"""Verify native package inputs include licenses before resource sealing."""
import importlib.util
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


if __name__ == '__main__':
    unittest.main()
