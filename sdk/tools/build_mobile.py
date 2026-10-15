#!/usr/bin/env python3
"""Stage the shared Rust C ABI for the Expo client from the repository root."""

import argparse
import os
import pathlib
import platform
import shutil
import subprocess

PROJECT = pathlib.Path(__file__).resolve().parents[2]
SDK = PROJECT / "sdk"


def build_android():
    ndk = os.environ.get("ANDROID_NDK_HOME")
    if not ndk:
        raise SystemExit("Set ANDROID_NDK_HOME to an installed Android NDK directory.")
    host = {"Linux": "linux-x86_64", "Darwin": "darwin-x86_64"}.get(platform.system())
    if not host:
        raise SystemExit("Build Android SDK artifacts from a Linux or macOS host.")
    toolchain = pathlib.Path(ndk) / "toolchains/llvm/prebuilt" / host / "bin"
    targets = [
        ("aarch64-linux-android", "aarch64-linux-android", "arm64-v8a"),
        ("x86_64-linux-android", "x86_64-linux-android", "x86_64"),
    ]
    subprocess.run(["rustup", "target", "add", *[row[0] for row in targets]], check=True)
    for target, compiler, abi in targets:
        linker = toolchain / f"{compiler}24-clang"
        if not linker.is_file():
            raise SystemExit("The configured NDK does not contain the required compiler.")
        environment = {
            **os.environ,
            f"CARGO_TARGET_{target.upper().replace('-', '_')}_LINKER": str(linker),
            f"CC_{target.replace('-', '_')}": str(linker),
            f"AR_{target.replace('-', '_')}": str(toolchain / "llvm-ar"),
        }
        subprocess.run([
            "cargo", "build", "--locked", "--release", "-p", "swarmie-client-ffi", "--target", target,
        ], cwd=PROJECT, env=environment, check=True)
        output = SDK / "react-native/android/src/main/jniLibs" / abi
        output.mkdir(parents=True, exist_ok=True)
        shutil.copy2(PROJECT / "target" / target / "release/libswarmie_client_ffi.so", output)


def build_ios():
    if platform.system() != "Darwin":
        raise SystemExit("iOS SDK builds require a configured macOS host and Xcode.")
    subprocess.run(["python3", str(SDK / "tools/build_apple.py"), "--release"], cwd=PROJECT, check=True)
    output = SDK / "react-native/native/CSwarmie.xcframework"
    output.parent.mkdir(parents=True, exist_ok=True)
    shutil.copytree(SDK / "swift/CSwarmie.xcframework", output, dirs_exist_ok=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("platform", choices=["android", "ios"])
    args = parser.parse_args()
    build_ios() if args.platform == "ios" else build_android()
