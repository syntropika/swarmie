#!/usr/bin/env python3
"""Verify that untrusted or mismatched SSH package streams cannot reach installation."""
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[3]
SCRIPT = ROOT / 'scripts/distribution/prepare-ssh.py'
SPEC = importlib.util.spec_from_file_location('ssh_prepare', SCRIPT)
PREPARE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PREPARE)


def archive(extra=None):
    output = io.BytesIO()
    with tarfile.open(fileobj=output, mode='w:gz') as bundle:
        for relative in sorted(PREPARE.FILES):
            content = b'fixture'
            member = tarfile.TarInfo('runtime/' + relative)
            member.size = len(content)
            bundle.addfile(member, io.BytesIO(content))
        if extra:
            member = tarfile.TarInfo(extra[0])
            if extra[1] == 'file':
                member.size = 1
                bundle.addfile(member, io.BytesIO(b'x'))
            else:
                member.type = tarfile.SYMTYPE if extra[1] == 'symlink' else tarfile.LNKTYPE
                member.linkname = '/outside'
                bundle.addfile(member)
    return output.getvalue()


class SshPreparationTests(unittest.TestCase):
    def test_archive_rejects_traversal_links_duplicates_and_extra_files(self):
        for member in [('runtime/../../escaped', 'file'), ('/escaped', 'file'),
                       ('runtime/link', 'symlink'), ('runtime/link', 'hardlink'),
                       ('runtime/install.py', 'file'), ('runtime/extra', 'file')]:
            with self.subTest(member=member), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                source = root / 'source.tar.gz'
                source.write_bytes(archive(member))
                target = root / 'extract'
                target.mkdir()
                with self.assertRaisesRegex(ValueError, 'invalid-package'):
                    PREPARE.extract(source, target)
                self.assertFalse((root / 'escaped').exists())

    def test_remote_checksum_and_platform_checks_precede_extraction_or_installation(self):
        data = archive()
        for checksum, target, error in [
            ('00' * 32, PREPARE.destination(), 'checksum-mismatch'),
            (hashlib.sha256(data).hexdigest(), 'unsupported-architecture', 'platform-changed'),
            (hashlib.sha256(data).hexdigest(), PREPARE.destination(), 'invalid-package'),
        ]:
            with self.subTest(error=error), tempfile.TemporaryDirectory() as temporary:
                home = Path(temporary)
                result = subprocess.run([sys.executable, SCRIPT, 'install', checksum, target, '0.1.0'],
                                        input=data, capture_output=True,
                                        env={**os.environ, 'HOME': str(home), 'PATH': '/usr/bin:/bin'})
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertEqual(json.loads(result.stdout)['error'], error)
                self.assertFalse((home / '.local').exists())
                self.assertFalse((home / '.swarmie').exists())


if __name__ == '__main__':
    unittest.main()
