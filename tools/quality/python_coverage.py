"""Use coverage.py's existing instrumenter for the quality tooling baseline."""

import argparse
from pathlib import Path
import subprocess
import sys

import coverage

from common import ROOT, digest, write_json


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", required=True)
    args = parser.parse_args()
    output = Path(args.output_dir).resolve()
    if output.exists() and any(output.iterdir()):
        parser.error("coverage output directory must be empty")
    output.mkdir(parents=True, exist_ok=True)
    write_json(output / "python-producer.json", {
        "coverage_version": coverage.__version__,
        "coverage_module_sha256": digest(Path(coverage.__file__)),
        "python_version": sys.version,
        "source_scope": "tools/quality",
        "subprocess_coverage": False,
    })
    data = str(output / "python.coverage")
    subprocess.run([sys.executable, "-m", "coverage", "run", "--data-file", data,
                    "--source", "tools/quality", "tools/quality/cases.py",
                    "--output", str(output / "critical-cases.json")], cwd=ROOT, check=True)
    subprocess.run([sys.executable, "-m", "coverage", "lcov", "--data-file", data,
                    "-o", str(output / "python.lcov")], cwd=ROOT, check=True)


if __name__ == "__main__":
    main()
