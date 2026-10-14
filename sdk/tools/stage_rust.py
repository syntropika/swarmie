#!/usr/bin/env python3
"""Stage a standalone native-client Rust workspace without private documentation."""
import pathlib
import shutil
import tomllib
import subprocess

PROJECT = pathlib.Path(__file__).resolve().parents[2]

def stage(destination):
    destination = pathlib.Path(destination)
    if destination.is_relative_to(PROJECT): raise ValueError('Use an external staging directory')
    destination.mkdir(parents=True, exist_ok=True)
    manifest = tomllib.loads((PROJECT / 'Cargo.toml').read_text())
    text = '[workspace]\nmembers = ["crates/client", "crates/client-ffi"]\nresolver = "2"\n\n[workspace.package]\n'
    import json
    for key, value in manifest['workspace']['package'].items(): text += key + ' = ' + json.dumps(value) + '\n'
    text += '\n[workspace.dependencies]\n'
    for key in ['serde', 'serde_json', 'tokio', 'uuid', 'futures-util']:
        value = manifest['workspace']['dependencies'][key]
        if isinstance(value, str): encoded = json.dumps(value)
        else: encoded = '{ ' + ', '.join(k + ' = ' + json.dumps(v) for k, v in value.items()) + ' }'
        text += key + ' = ' + encoded + '\n'
    (destination / 'Cargo.toml').write_text(text)
    shutil.copy2(PROJECT / 'Cargo.lock', destination)
    shutil.copy2(PROJECT / 'mise.toml', destination)
    for crate in ['client', 'client-ffi']:
        shutil.copytree(PROJECT / 'crates' / crate, destination / 'crates' / crate)
    shutil.copytree(PROJECT / 'sdk/wit', destination / 'sdk/wit')
    shutil.copytree(PROJECT / 'sdk/swift', destination / 'sdk/swift', ignore=shutil.ignore_patterns('.build', '.swiftpm', 'native', 'CSwarmie.xcframework'))
    (destination / 'sdk/tools').mkdir()
    shutil.copy2(PROJECT / 'sdk/tools/build_apple.py', destination / 'sdk/tools')
    # Cargo prunes unrelated workspace members while retaining the locked versions.
    subprocess.run(['cargo', 'metadata', '--offline', '--format-version', '1', '--manifest-path', str(destination / 'Cargo.toml')], check=True, stdout=subprocess.DEVNULL)

if __name__ == '__main__':
    import argparse
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('destination', type=pathlib.Path)
    stage(parser.parse_args().destination.resolve())
