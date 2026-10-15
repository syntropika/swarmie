#!/usr/bin/env python3
"""Verify SDK assets survive packaging and installation without provider requests."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import patch
from agent_tools import copy_agent_tools, digest, stage_auth_worker


def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(filename))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


INSTALL = load('agent_install', 'install-runtime.py')
PREPARE = load('agent_prepare', 'prepare-ssh.py')
PACKAGE = load('agent_package', 'package-runtime.py')
DESKTOP = load('agent_desktop_package', 'package.py')
STAGE = load('agent_stage', 'stage-agent-tools.py')
ROOT = Path(__file__).resolve().parents[2]


class AgentToolDistribution(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.support = self.root / 'support'
        for relative, value in [('claude/worker.mjs', 'SDK worker fixture'), ('claude/node_modules/sdk/index.js', 'SDK dependency fixture'), ('cursor/bin/cursor-sdk-bridge', 'bridge fixture')]:
            target = self.support / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(value)
        (self.support / 'cursor/bin/cursor-sdk-bridge').chmod(0o755)
        self.inventory = {path.relative_to(self.support).as_posix(): digest(path) for path in self.support.rglob('*') if path.is_file()}
        (self.support / 'agent-tools.json').write_text(json.dumps({'kind': 'swarmie-agent-tools', 'platform': 'linux-x86_64', 'files': self.inventory}))

    def package(self):
        package = self.root / 'package'
        (package / 'bin').mkdir(parents=True)
        (package / 'bin/swarmie').write_text('runtime fixture')
        (package / 'bin/swarmie').chmod(0o755)
        for filename in ('LICENSE', 'NOTICE'):
            shutil_source = ROOT / filename
            (package / filename).write_bytes(shutil_source.read_bytes())
        (package / 'install.py').write_text('installer fixture')
        support = copy_agent_tools(self.support, package / 'agent-tools', 'linux-x86_64')
        (package / 'runtime-package.json').write_text(json.dumps({'kind': 'swarmie-runtime', 'platform': 'linux-x86_64', 'version': '0.1.0', 'executableSha256': digest(package / 'bin/swarmie'), 'supportFiles': support}))
        return package

    def arguments(self, package):
        return argparse.Namespace(package=package, prefix=self.root / 'installed', bin_dir=self.root / 'bin', uninstall=False, if_missing=False)

    def test_copy_rejects_modified_symlinked_and_wrong_platform_assets(self):
        with self.assertRaisesRegex(SystemExit, 'platform'):
            copy_agent_tools(self.support, self.root / 'wrong', 'macos-aarch64')
        worker = self.support / 'claude/worker.mjs'
        worker.write_text('modified')
        with self.assertRaisesRegex(SystemExit, 'SHA-256'):
            copy_agent_tools(self.support, self.root / 'changed')
        worker.unlink()
        worker.symlink_to(ROOT / 'Cargo.toml')
        with self.assertRaisesRegex(SystemExit, 'regular files'):
            copy_agent_tools(self.support, self.root / 'linked')

    def test_claude_staging_ships_all_runtime_modules_and_excludes_tests(self):
        project = self.root / 'source'
        source = project / 'packages/claude-agent-worker'
        source.mkdir(parents=True)
        for name in ('worker.mjs', 'session.mjs', 'diagnostics.mjs', 'transport.mjs'):
            (source / name).write_text('runtime module fixture')
        (source / 'package.json').write_text(json.dumps({'dependencies': {'@anthropic-ai/claude-agent-sdk': '0.3.299'}}))
        (source / 'package-lock.json').write_text('{}')
        (source / 'test').mkdir()
        (source / 'test/unit.mjs').write_text('not a runtime asset')
        output = self.root / 'new-staged-support'
        def npm(command, cwd, check):
            self.assertIn('--omit=optional', command)
            dependency = cwd / 'node_modules/@anthropic-ai/claude-agent-sdk/sdk.mjs'
            dependency.parent.mkdir(parents=True)
            dependency.write_text('SDK dependency fixture')
        with patch.object(STAGE, 'PROJECT', project), patch.object(STAGE.platform, 'system', return_value='Linux'), patch.object(STAGE.platform, 'machine', return_value='x86_64'), patch.object(STAGE.subprocess, 'check_output', return_value='v22.0.0'), patch.object(STAGE.subprocess, 'run', side_effect=npm), patch('sys.argv', ['stage-agent-tools.py', '--tools', 'claude', '--without-auth', '--output', str(output)]):
            STAGE.main()
        for name in ('worker.mjs', 'session.mjs', 'diagnostics.mjs', 'transport.mjs'):
            self.assertTrue((output / 'claude' / name).is_file())
        self.assertFalse((output / 'claude/test').exists())
        copied = self.root / 'verified-staged-support'
        inventory = copy_agent_tools(output, copied, 'linux-x86_64')
        self.assertIn('agent-tools/claude/transport.mjs', inventory)

    def test_auth_support_stages_locked_dependencies_and_excludes_test_modules(self):
        source = self.root / 'auth-source'
        source.mkdir()
        for name in ('worker.mjs', 'worker.test.mjs'):
            (source / name).write_text('authentication worker fixture')
        (source / 'package.json').write_text(json.dumps({'dependencies': {'@cursor/sdk': '1.0.37', '@earendil-works/pi-ai': '1.2.0'}}))
        (source / 'package-lock.json').write_text('{}')
        destination = self.root / 'auth'
        def npm(command, cwd, check):
            self.assertIn('--ignore-scripts', command)
            dependency = cwd / 'node_modules/@cursor/sdk/index.js'
            dependency.parent.mkdir(parents=True)
            dependency.write_text('locked dependency fixture')
        with patch('agent_tools.subprocess.check_output', return_value='v22.13.0'), patch('agent_tools.subprocess.run', side_effect=npm):
            versions = stage_auth_worker(source, destination)
        self.assertEqual(versions['@cursor/sdk'], '1.0.37')
        self.assertTrue((destination / 'node_modules/@cursor/sdk/index.js').is_file())
        self.assertFalse((destination / 'worker.test.mjs').exists())
        with patch('agent_tools.subprocess.check_output', return_value='v22.12.0'):
            with self.assertRaisesRegex(SystemExit, '22.13'):
                stage_auth_worker(source, self.root / 'too-old')

    def test_installer_tracks_support_updates_and_preserves_user_content(self):
        package = self.package()
        args = self.arguments(package)
        with patch.object(INSTALL.platform, 'system', return_value='Linux'), patch.object(INSTALL.platform, 'machine', return_value='x86_64'), patch.object(INSTALL, 'service_units', return_value=[]):
            INSTALL.install(args)
            worker = args.prefix / 'agent-tools/claude/worker.mjs'
            self.assertEqual(worker.read_text(), 'SDK worker fixture')
            self.assertTrue(os.access(args.prefix / 'agent-tools/cursor/bin/cursor-sdk-bridge', os.X_OK))
            user = args.prefix / 'agent-tools/claude/user-note'
            user.write_text('user content')
            empty = args.prefix / 'agent-tools/claude/user-directory'
            empty.mkdir()
            worker.write_text('modified locally')
            with self.assertRaisesRegex(SystemExit, 'modified installation file'):
                INSTALL.install(args)
            worker.write_text('SDK worker fixture')
            metadata = json.loads((package / 'runtime-package.json').read_text())
            replacement = package / 'agent-tools/claude/worker.mjs'
            replacement.write_text('updated SDK worker')
            metadata['supportFiles']['agent-tools/claude/worker.mjs'] = digest(replacement)
            (package / 'runtime-package.json').write_text(json.dumps(metadata))
            INSTALL.install(args)
            self.assertEqual(worker.read_text(), 'updated SDK worker')
            args.uninstall = True
            INSTALL.install(args)
            self.assertEqual(user.read_text(), 'user content')
            self.assertTrue(empty.is_dir())
            self.assertFalse(worker.exists())

    def test_remote_extraction_requires_declared_checksum_and_preserves_executable_mode(self):
        package = self.package()
        archive = self.root / 'package.tar.gz'
        with tarfile.open(archive, 'w:gz') as bundle:
            bundle.add(package, arcname='runtime')
        output = self.root / 'extracted'
        output.mkdir()
        PREPARE.extract(archive, output)
        self.assertEqual((output / 'agent-tools/claude/worker.mjs').read_text(), 'SDK worker fixture')
        self.assertTrue(os.access(output / 'agent-tools/cursor/bin/cursor-sdk-bridge', os.X_OK))
        (package / 'agent-tools/claude/worker.mjs').write_text('tampered')
        with tarfile.open(archive, 'w:gz') as bundle:
            bundle.add(package, arcname='runtime')
        refused = self.root / 'refused'
        refused.mkdir()
        with self.assertRaisesRegex(ValueError, 'invalid-package'):
            PREPARE.extract(archive, refused)

    def test_bridge_extraction_rejects_escape_and_links(self):
        for name, kind in [('../escape', tarfile.REGTYPE), ('bin/bridge', tarfile.SYMTYPE), ('bin/..\\escape', tarfile.DIRTYPE), ('C:/escape', tarfile.DIRTYPE)]:
            archive = self.root / 'malicious.tar.gz'
            with tarfile.open(archive, 'w:gz') as bundle:
                member = tarfile.TarInfo(name)
                member.type = kind
                member.linkname = '/tmp/outside'
                bundle.addfile(member)
            with self.assertRaisesRegex(SystemExit, 'Invalid Cursor Bridge'):
                STAGE.extract_bridge(archive, self.root / 'bridge')

    def test_runtime_packaging_includes_support_and_sha_inventory(self):
        binary = self.root / 'swarmie'
        binary.write_text('runtime fixture')
        output = self.root / 'output'
        with patch.object(PACKAGE.platform, 'system', return_value='Linux'), patch.object(PACKAGE.platform, 'machine', return_value='x86_64'), patch.object(PACKAGE.subprocess, 'check_output', return_value='swarmie 0.1.0'), patch('sys.argv', ['package-runtime.py', '--binary', str(binary), '--output', str(output), '--agent-tools', str(self.support)]):
            PACKAGE.main()
        bundle = output / 'swarmie-runtime-0.1.0-linux-x86_64'
        metadata = json.loads((bundle / 'runtime-package.json').read_text())
        for relative, expected in metadata['supportFiles'].items():
            self.assertEqual(digest(bundle / relative), expected)

    def test_macos_sdk_assets_are_packaging_inputs_before_signing(self):
        assets = self.root / 'frontend'
        assets.mkdir()
        (assets / 'index.html').write_text('frontend fixture')
        command = ['native', 'package', '--assets', assets, '--signing', 'identity']
        support_manifest = self.support / 'agent-tools.json'
        metadata = json.loads(support_manifest.read_text())
        metadata['platform'] = 'macos-aarch64'
        support_manifest.write_text(json.dumps(metadata))
        seen = []
        def package(arguments, cwd):
            staged = Path(arguments[arguments.index('--assets') + 1])
            self.assertEqual((staged / 'agent-tools/claude/worker.mjs').read_text(), 'SDK worker fixture')
            self.assertIn('Apache License', (staged / 'licenses/swarmie/LICENSE').read_text())
            seen.append(staged)
        with patch.object(DESKTOP, 'run', package):
            DESKTOP.package_native(command, self.root, assets, 'macos', self.support, 'macos-aarch64')
        self.assertEqual(len(seen), 1)
        self.assertFalse(seen[0].exists())
        self.assertFalse((assets / 'agent-tools').exists())

    def test_npm_launcher_passes_runtime_owned_worker_root_and_node(self):
        node = subprocess.check_output(['node', '-p', 'process.execPath'], text=True).strip()
        package = self.root / 'npm'
        (package / 'bin').mkdir(parents=True)
        (package / 'native').mkdir()
        (package / 'bin/swarmie.cjs').write_bytes((ROOT / 'packages/runtime/bin/swarmie.cjs').read_bytes())
        target = json.loads(subprocess.check_output(['node', '-p', 'JSON.stringify({platform:process.platform,arch:process.arch})'], text=True))
        (package / 'native/target.json').write_text(json.dumps(target))
        executable = package / 'native/swarmie'
        executable.write_text('#!/usr/bin/env node\nconsole.log(JSON.stringify({root:process.env.SWARMIE_AGENT_TOOLS_DIR,node:process.env.SWARMIE_NODE_EXECUTABLE,args:process.argv.slice(2)}));\n')
        executable.chmod(0o755)
        copy_agent_tools(self.support, package / 'agent-tools')
        environment = {key: value for key, value in os.environ.items() if key not in ('SWARMIE_AGENT_TOOLS_DIR', 'SWARMIE_NODE_EXECUTABLE')}
        result = json.loads(subprocess.check_output([node, str(package / 'bin/swarmie.cjs'), '--version'], text=True, env=environment))
        self.assertEqual(Path(result['root']).resolve(), package / 'agent-tools')
        self.assertEqual(result['node'], node)
        self.assertEqual(result['args'], ['--version'])


if __name__ == '__main__':
    unittest.main()
