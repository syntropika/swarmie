#!/usr/bin/env python3
"""Build a static XCFramework for the Swift SDK on a macOS build host."""
import argparse
import os
import pathlib
import platform
import re
import shutil
import subprocess
import tempfile

PROJECT = pathlib.Path(__file__).resolve().parents[2]
SDK = PROJECT / 'sdk'
TARGETS = ['aarch64-apple-darwin', 'x86_64-apple-darwin', 'aarch64-apple-ios', 'aarch64-apple-ios-sim', 'x86_64-apple-ios']

def run(command, **options): subprocess.run(command, check=True, **options)

def build(release=False):
    if platform.system() != 'Darwin': raise SystemExit('XCFramework builds require macOS and Xcode.')
    toolchain = re.search(r'^rust = "([^"]+)"', (PROJECT / 'mise.toml').read_text(), re.MULTILINE).group(1)
    environment = {**os.environ, 'RUSTUP_TOOLCHAIN': toolchain, 'MACOSX_DEPLOYMENT_TARGET': '13.0', 'CARGO_PROFILE_RELEASE_STRIP': 'none'}
    # A global iOS deployment target also affects host proc-macro dylibs.
    # Keep the host macOS target and set C compiler deployment flags per target.
    environment.pop('IPHONEOS_DEPLOYMENT_TARGET', None)
    for target in TARGETS[2:]:
        flag = '-mios-simulator-version-min=16.0' if target.endswith('-ios-sim') or target.startswith('x86_64') else '-mios-version-min=16.0'
        key = 'CFLAGS_' + target.replace('-', '_')
        environment[key] = environment.get(key, '') + ' ' + flag
    profile = 'release' if release else 'debug'
    run(['rustup', 'target', 'add', *TARGETS], env=environment)
    for target in TARGETS:
        command = ['cargo', 'build', '--locked', '-p', 'swarmie-client-ffi', '--target', target]
        if release: command.append('--release')
        run(command, cwd=PROJECT, env=environment)
    output = SDK / 'swift/CSwarmie.xcframework'
    with tempfile.TemporaryDirectory(prefix='swarmie-xcframework-') as directory:
        stage = pathlib.Path(directory)
        headers = stage / 'headers'
        headers.mkdir()
        shutil.copy2(SDK / 'wit/swarmie_client.h', headers)
        (headers / 'CSwarmie.h').write_text('''#include "swarmie_client.h"
static inline int swarmie_binding_load(const char *path) { (void)path; return swarmie_client_abi_version() == 1; }
static inline uint32_t swarmie_binding_version(void) { return swarmie_client_abi_version(); }
static inline char *swarmie_binding_execute(const char *command) { return swarmie_client_execute(command); }
static inline void swarmie_binding_free(char *result) { swarmie_client_free(result); }
''')
        (headers / 'module.modulemap').write_text('module CSwarmie { header "CSwarmie.h" export * }\n')
        libraries = []
        for name, targets in [('macos', TARGETS[:2]), ('ios', TARGETS[2:3]), ('simulator', TARGETS[3:])]:
            library = stage / name / 'libswarmie_client_ffi.a'
            library.parent.mkdir()
            sources = [PROJECT / 'target' / target / profile / 'libswarmie_client_ffi.a' for target in targets]
            if len(sources) > 1: run(['lipo', '-create', *map(str, sources), '-output', str(library)])
            else: shutil.copy2(sources[0], library)
            libraries.extend(['-library', str(library), '-headers', str(headers)])
        if output.exists(): shutil.rmtree(output)
        run(['xcodebuild', '-create-xcframework', *libraries, '-output', str(output)])
    print('Swift XCFramework staged for macOS, iOS, and iOS Simulator')

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--release', action='store_true')
    build(parser.parse_args().release)
