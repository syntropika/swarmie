"""Platform wheel tagging for a Rust library that does not use Python's ABI."""
import pathlib
import sys
from setuptools import setup
from setuptools.dist import Distribution
from wheel.bdist_wheel import bdist_wheel

suffix = {'darwin': 'dylib', 'win32': 'dll'}.get(sys.platform, 'so')
name = ('' if sys.platform == 'win32' else 'lib') + 'swarmie_client_ffi.' + suffix
if not (pathlib.Path(__file__).parent / 'swarmie_client/native' / name).is_file():
    raise RuntimeError('Native library is missing. Run just sdk-build from the source root before packaging.')

class NativeDistribution(Distribution):
    def has_ext_modules(self): return True

class NativeWheel(bdist_wheel):
    def get_tag(self):
        _, _, platform = super().get_tag()
        return 'py3', 'none', platform

setup(distclass=NativeDistribution, cmdclass={'bdist_wheel': NativeWheel})
