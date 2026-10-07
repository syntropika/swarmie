#!/usr/bin/env python3
"""Extract Ubuntu fixture dependencies locally; never install system packages."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

PACKAGES = ['wmctrl', 'openbox', 'libobrender32v5', 'libobt2v5', 'libimlib2t64',
            'python3-pyatspi', 'imagemagick-6.q16', 'libmagickcore-6.q16-7t64',
            'libmagickwand-6.q16-7t64', 'imagemagick-6-common', 'liblqr-1-0', 'libraw23t64']


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
    (vendor / 'bin').mkdir(exist_ok=True)
    for name, target in [('python3', Path('/usr/bin/python3')), ('import', vendor / 'usr/bin/import-im6.q16'),
                         ('identify', vendor / 'usr/bin/identify-im6.q16'), ('convert', vendor / 'usr/bin/convert-im6.q16'),
                         ('wmctrl', vendor / 'usr/bin/wmctrl'), ('openbox', vendor / 'usr/bin/openbox')]:
        path = vendor / 'bin' / name
        if not path.exists():
            path.symlink_to(target)
    receipt.parent.mkdir(parents=True, exist_ok=True)
    receipt.write_text(json.dumps({'versions': versions, 'packageSha256': hashes, 'systemInstalled': False}, indent=2) + '\n')


if __name__ == '__main__':
    main()
