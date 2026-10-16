#!/usr/bin/env python3
"""Build and stage native SDK libraries for the current host from the root workspace."""
import argparse
import pathlib
import platform
import os
import shutil
import subprocess

PROJECT = pathlib.Path(__file__).resolve().parents[2]
SDK = PROJECT / 'sdk'

def build(release=False, apple=False):
    command = ['cargo', 'build', '--locked', '-p', 'swarmie-client-ffi', '-p', 'swarmie-client-node']
    if release: command.append('--release')
    environment = dict(os.environ)
    if platform.system() == 'Darwin':
        # Rust's strip path can misalign Mach-O string pools, including when
        # debug information is disabled through a development profile override.
        environment['CARGO_PROFILE_DEV_STRIP'] = 'none'
        environment['CARGO_PROFILE_RELEASE_STRIP'] = 'none'
        environment['MACOSX_DEPLOYMENT_TARGET'] = '13.0'
    subprocess.run(command, cwd=PROJECT, env=environment, check=True)
    target = PROJECT / 'target' / ('release' if release else 'debug')
    suffix = {'Linux': 'so', 'Darwin': 'dylib', 'Windows': 'dll'}[platform.system()]
    prefix = '' if suffix == 'dll' else 'lib'
    shared = prefix + 'swarmie_client_ffi.' + suffix
    node = prefix + 'swarmie_client_node.' + suffix
    for owner, name in [(SDK / 'typescript', 'native'), (SDK / 'python/swarmie_client', 'native'), (SDK / 'swift/Sources/SwarmieClient', 'native')]:
        directory = owner / name
        directory.mkdir(exist_ok=True)
        artifact = target / (node if owner.name == 'typescript' else shared)
        shutil.copy2(artifact, directory / ('swarmie_client.node' if owner.name == 'typescript' else shared))
    go = SDK / 'go/native'
    shutil.copy2(target / ('swarmie_client_ffi.lib' if suffix == 'dll' else 'libswarmie_client_ffi.a'), go)
    shutil.copy2(SDK / 'wit/swarmie_client.h', go / 'swarmie_client.h')
    for directory in [SDK / 'typescript/wit', SDK / 'python/swarmie_client/wit', SDK / 'go/wit', SDK / 'swift/wit']:
        directory.mkdir(exist_ok=True)
        shutil.copy2(SDK / 'wit/client.wit', directory / 'client.wit')
    if apple:
        subprocess.run(['python3', str(SDK / 'tools/build_apple.py'), *(['--release'] if release else [])], cwd=PROJECT, check=True)
    print('Native SDK libraries staged for ' + platform.system() + ' ' + platform.machine())

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--release', action='store_true')
    parser.add_argument('--apple', action='store_true', help='Also stage the macOS/iOS static XCFramework; requires macOS')
    args = parser.parse_args()
    build(args.release, args.apple)
