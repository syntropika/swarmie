#!/usr/bin/env python3
"""Extract Ubuntu fixture dependencies locally; never install system packages."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

PACKAGES = ['tesseract-ocr', 'tesseract-ocr-eng', 'tesseract-ocr-osd', 'libtesseract5', 'liblept5']


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--receipt', type=Path, required=True)
    args = parser.parse_args()
    fixture = Path(__file__).resolve().parent
    root = fixture.parents[2]
    receipt = args.receipt.resolve()
    if receipt == root or root in receipt.parents:
        parser.error('The package receipt must be outside the repository')
    vendor = fixture / 'vendor'
    packages = vendor / 'packages'
    packages.mkdir(parents=True, exist_ok=True)
    versions = {}
    for package in PACKAGES:
        output = subprocess.check_output(['apt-cache', 'policy', package], text=True)
        version = next(line.split(':', 1)[1].strip() for line in output.splitlines() if 'Candidate:' in line)
        if version == '(none)':
            raise RuntimeError('No distro package candidate for ' + package)
        versions[package] = version
    subprocess.run(['apt', 'download', *[name + '=' + version for name, version in versions.items()]], cwd=packages, check=True)
    hashes = {}
    for package in sorted(packages.glob('*.deb')):
        subprocess.run(['dpkg-deb', '-x', str(package), str(vendor)], check=True)
        hashes[package.name] = hashlib.sha256(package.read_bytes()).hexdigest()
    receipt.parent.mkdir(parents=True, exist_ok=True)
    receipt.write_text(json.dumps({'versions': versions, 'packageSha256': hashes, 'systemInstalled': False}, indent=2) + '\n')


if __name__ == '__main__':
    main()
