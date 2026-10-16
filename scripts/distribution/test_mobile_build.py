"""Native build decisions must use successful signed builds and actual archive runtimes."""

from pathlib import Path
import tempfile
import unittest
import zipfile

from mobile_build import ARTIFACT_NAME, WORKFLOW_PATH, baseline, plan, record_binary


class ArtifactSource:
    def __init__(self, *, event="workflow_dispatch", conclusion="success", commit="a" * 40):
        self.metadata = {
            "event": event, "conclusion": conclusion, "path": WORKFLOW_PATH,
            "head_branch": "release", "head_sha": "a" * 40,
        }
        self.value = {"platform": "ios", "variant": "development", "commit": commit,
                      "runtimeVersion": "b" * 40}

    def candidates(self):
        return [{"id": 1, "name": ARTIFACT_NAME, "expired": False,
                 "workflow_run": {"id": 2, "head_branch": "release"}}]

    def run(self, _run_id):
        return self.metadata

    def record(self, _artifact):
        return self.value


class MobileBuildTest(unittest.TestCase):
    def test_first_run_and_native_changes_require_a_binary(self):
        current = {"runtimeVersion": "a" * 40}
        self.assertTrue(plan(current, None)[0])
        self.assertTrue(plan(current, {"runtimeVersion": "b" * 40})[0])
        self.assertFalse(plan(current, current)[0])

    def test_pull_request_failed_and_other_branch_artifacts_are_not_baselines(self):
        self.assertIsNone(baseline(ArtifactSource(event="pull_request"), "release"))
        self.assertIsNone(baseline(ArtifactSource(conclusion="failure"), "release"))
        self.assertIsNone(baseline(ArtifactSource(), "other"))
        self.assertEqual(baseline(ArtifactSource(), "release")["runtimeVersion"], "b" * 40)

    def test_stale_or_malformed_baselines_fail_closed(self):
        with self.assertRaises(ValueError):
            baseline(ArtifactSource(commit="c" * 40), "release")
        with self.assertRaises(ValueError):
            plan({"runtimeVersion": {"policy": "fingerprint"}}, None)

    def test_record_requires_the_archived_runtime_to_match_the_linux_plan(self):
        with tempfile.TemporaryDirectory() as directory:
            ipa = Path(directory) / "app.ipa"
            with zipfile.ZipFile(ipa, "w") as archive:
                archive.writestr("Payload/Swarmie.app/EXUpdates.bundle/fingerprint", "b" * 40)
            record = record_binary(ipa, "b" * 40, "a" * 40)
            self.assertEqual(record["runtimeVersion"], "b" * 40)
            with self.assertRaises(ValueError):
                record_binary(ipa, "c" * 40, "a" * 40)


if __name__ == "__main__":
    unittest.main()
