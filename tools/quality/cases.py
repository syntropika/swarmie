"""Record individual unittest outcomes against explicit critical behavior selectors."""

import argparse
import json
from pathlib import Path
import unittest

from common import ROOT, digest, write_json


class Outcomes(unittest.TextTestResult):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.outcomes = {}

    def addSuccess(self, test):
        self.outcomes[test.id()] = "passed"
        super().addSuccess(test)

    def addFailure(self, test, error):
        self.outcomes[test.id()] = "failed"
        super().addFailure(test, error)

    def addError(self, test, error):
        self.outcomes[test.id()] = "error"
        super().addError(test, error)

    def addSkip(self, test, reason):
        self.outcomes[test.id()] = "skipped"
        super().addSkip(test, reason)


def trace(manifest, outcomes):
    return {criterion: {branch: {case: outcomes.get(case, "not_executed") for case in cases}
                        for branch, cases in branches.items()}
            for criterion, branches in manifest["criteria"].items()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True)
    args = parser.parse_args()
    path = ROOT / "tools/quality/critical-cases.json"
    manifest = json.loads(path.read_text())
    suite = unittest.defaultTestLoader.discover(str(ROOT / "tools/quality"), "test_*.py")
    result = unittest.TextTestRunner(verbosity=2, resultclass=Outcomes).run(suite)
    mapped = trace(manifest, result.outcomes)
    missing = [case for branches in mapped.values() for cases in branches.values()
               for case, outcome in cases.items() if outcome != "passed"]
    write_json(args.output, {"schema_version": 1, "manifest_sha256": digest(path),
                            "scope": manifest["scope"], "cases": result.outcomes,
                            "critical_behavior": mapped, "acceptance": "not_determined",
                            "limitations": manifest["limitations"]})
    raise SystemExit(not result.wasSuccessful() or bool(missing))


if __name__ == "__main__":
    main()
