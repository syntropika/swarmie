"""Platform wheel tagging for a Rust library that does not use Python's ABI."""
from setuptools import setup
from setuptools.dist import Distribution
from wheel.bdist_wheel import bdist_wheel

class NativeDistribution(Distribution):
    def has_ext_modules(self): return True

class NativeWheel(bdist_wheel):
    def get_tag(self):
        _, _, platform = super().get_tag()
        return 'py3', 'none', platform

setup(distclass=NativeDistribution, cmdclass={'bdist_wheel': NativeWheel})
