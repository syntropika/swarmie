#!/usr/bin/env python3
"""Verify release completeness and integrity before assets become publishable."""
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location('swarmie_release', Path(__file__).with_name('release.py'))
RELEASE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RELEASE)
VERSION = '1.2.3'
TAG = 'v' + VERSION
REPOSITORY = 'example/swarmie'


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.inputs = self.root / 'inputs'
        self.output = self.root / 'release'
        (self.root / 'apps/desktop').mkdir(parents=True)
        (self.root / 'Cargo.toml').write_text(f'[workspace.package]\nversion = "{VERSION}"\n')
        for name in ('app.json', 'package.json'):
            (self.root / 'apps/desktop' / name).write_text(json.dumps({'version': VERSION}))
        self.root_patch = patch.object(RELEASE, 'ROOT', self.root)
        self.root_patch.start()
        self.addCleanup(self.root_patch.stop)
        self.records = {}
        for kind, manifest_name in [('desktop', 'manifest.json'), ('runtime', 'runtime-manifest.json')]:
            for platform in (RELEASE.RUNTIME_PLATFORMS if kind == "runtime" else RELEASE.PLATFORMS):
                directory = self.inputs / platform / kind
                directory.mkdir(parents=True)
                extension = '.tar.gz' if kind == 'runtime' or platform.startswith('linux-') else '.zip'
                name = f'swarmie{"-runtime" if kind == "runtime" else ""}-{VERSION}-{platform}{extension}'
                archive = directory / name
                archive.write_bytes(f'{kind}:{platform}:fixture archive'.encode())
                checksum = hashlib.sha256(archive.read_bytes()).hexdigest()
                sidecar = directory / (name + '.sha256')
                sidecar.write_text(checksum + '  ' + name + '\n')
                manifest = directory / manifest_name
                manifest.write_text(json.dumps({'version': VERSION, 'artifacts': {platform: {
                    'url': f'https://github.com/{REPOSITORY}/releases/download/{TAG}/{name}',
                    'sha256': checksum,
                    'fileName': name,
                }}}))
                self.records[kind, platform] = (manifest, archive, sidecar)

    def assemble(self):
        return RELEASE.assemble(self.inputs, self.output, REPOSITORY, TAG, RELEASE.PLATFORMS, RELEASE.RUNTIME_PLATFORMS)

    def reject(self):
        with self.assertRaises(ValueError):
            self.assemble()
        self.assertFalse(self.output.exists(), 'Rejected inputs must never create release output')

    def mutate_manifest(self, change):
        path, _, _ = self.records['runtime', 'linux-aarch64']
        manifest = json.loads(path.read_text())
        change(manifest)
        path.write_text(json.dumps(manifest))

    def test_complete_multi_platform_release_preserves_only_verified_inputs(self):
        (self.inputs / 'private-notes.txt').write_text('Excluded input')
        result = self.assemble()
        self.assertEqual(result['assets'], 27)
        self.assertEqual(len(list(self.output.iterdir())), 27)
        for name in ('manifest.json', 'runtime-manifest.json'):
            merged = json.loads((self.output / name).read_text())
            self.assertEqual(merged['version'], VERSION)
            self.assertEqual(set(merged['artifacts']), set(RELEASE.RUNTIME_PLATFORMS if name == 'runtime-manifest.json' else RELEASE.PLATFORMS))
        for _, archive, sidecar in self.records.values():
            self.assertEqual((self.output / archive.name).read_bytes(), archive.read_bytes())
            self.assertEqual((self.output / sidecar.name).read_bytes(), sidecar.read_bytes())
        checksums = (self.output / 'SHA256SUMS').read_text().splitlines()
        self.assertEqual(len(checksums), 26)
        for line in checksums:
            checksum, name = line.split()
            self.assertEqual(checksum, hashlib.sha256((self.output / name).read_bytes()).hexdigest())
        self.assertFalse((self.output / 'private-notes.txt').exists())

    def test_existing_output_is_never_overwritten(self):
        self.output.mkdir()
        sentinel = self.output / 'user-file'
        sentinel.write_text('Preserved')
        with self.assertRaises(FileExistsError):
            self.assemble()
        self.assertEqual(list(self.output.iterdir()), [sentinel])
        self.assertEqual(sentinel.read_text(), 'Preserved')

    def test_missing_platform_is_rejected(self):
        self.records['runtime', 'macos-aarch64'][0].unlink()
        self.reject()

    def test_duplicate_platform_is_rejected(self):
        manifest = self.records['desktop', 'linux-x86_64'][0]
        duplicate = self.inputs / 'duplicate'
        duplicate.mkdir()
        (duplicate / manifest.name).write_bytes(manifest.read_bytes())
        self.reject()

    def test_wrong_manifest_version_is_rejected(self):
        self.mutate_manifest(lambda m: m.update(version='1.2.4'))
        self.reject()

    def test_wrong_repository_or_latest_url_is_rejected(self):
        self.mutate_manifest(lambda m: m['artifacts']['linux-aarch64'].update(url='https://example.com/latest.tar.gz'))
        self.reject()

    def test_path_traversal_is_rejected(self):
        self.mutate_manifest(lambda m: m['artifacts']['linux-aarch64'].update(fileName='../runtime.tar.gz'))
        self.reject()

    def test_changed_archive_is_rejected(self):
        self.records['runtime', 'linux-aarch64'][1].write_bytes(b'changed bytes')
        self.reject()

    def test_changed_manifest_checksum_is_rejected(self):
        self.mutate_manifest(lambda m: m['artifacts']['linux-aarch64'].update(sha256='0' * 64))
        self.reject()

    def test_sidecar_names_the_verified_archive(self):
        _, archive, sidecar = self.records['runtime', 'linux-aarch64']
        sidecar.write_text(hashlib.sha256(archive.read_bytes()).hexdigest() + '  another-file.tar.gz\n')
        self.reject()

    def test_linked_archive_is_rejected(self):
        _, archive, _ = self.records['runtime', 'linux-aarch64']
        target = archive.with_name('linked-input')
        archive.rename(target)
        archive.symlink_to(target)
        self.reject()

    def test_unrecognized_fields_are_rejected(self):
        self.mutate_manifest(lambda m: m.update(extra='unexpected'))
        self.reject()

    def test_unexpected_platform_is_rejected(self):
        self.mutate_manifest(lambda m: m['artifacts'].update({'unsupported-x86_64': {}}))
        self.reject()

    def test_non_object_manifest_is_rejected(self):
        self.records['runtime', 'linux-aarch64'][0].write_text('[]')
        self.reject()

    def test_tag_and_all_package_versions_must_match(self):
        self.assertEqual(RELEASE.validate_tag(TAG), VERSION)
        with self.assertRaises(ValueError):
            RELEASE.validate_tag('v1.2.4')
        for name in ('app.json', 'package.json'):
            with self.subTest(name=name):
                path = self.root / 'apps/desktop' / name
                path.write_text('{"version":"1.2.4"}')
                with self.assertRaises(ValueError):
                    RELEASE.validate_tag(TAG)
                path.write_text(json.dumps({'version': VERSION}))


if __name__ == '__main__':
    unittest.main()
