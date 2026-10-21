"""Behavioral checks for fail-safe selection, graph rules and trustworthy evidence."""

import copy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
import local
import cases
import runtime_trace
import hashlib

import architecture
from common import ROOT, git
import line_coverage
import evidence
import plan


class Repository(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name) / "source"
        self.root.mkdir()
        git(self.root, "init", "-q")
        git(self.root, "config", "user.name", "Quality Fixture")
        git(self.root, "config", "user.email", "fixture@example.invalid")

    def commit(self):
        git(self.root, "add", ".")
        git(self.root, "commit", "-qm", "test: update fixture")
        return git(self.root, "rev-parse", "HEAD").decode().strip()


class SelectionTests(Repository):
    def test_known_families_and_foundational_changes(self):
        for path, expected in (("plugins/agent-loop-default/src/engine.rs", {"foundation": True, "frontend": False}),
                               ("apps/web/src/app.tsx", {"foundation": False, "frontend": True}),
                               ("Cargo.lock", {"foundation": True, "frontend": True}),
                               (".github/workflows/new.yml", {"foundation": True, "frontend": True}),
                               ("plugins/new/Cargo.toml", {"foundation": True, "frontend": True}),
                               ("unknown/new.rs", {"foundation": True, "frontend": True})):
            self.assertEqual(plan.classify([path])[0], expected, path)

    def test_missing_history_runs_every_family(self):
        (self.root / "a").write_text("a")
        self.commit()
        result = plan.plan(self.root, {}, "bad-ref")
        self.assertTrue(result["fallback"])
        self.assertTrue(all(result["families"].values()))

    def test_rename_selects_both_sides_and_recovers_after_valid_base(self):
        path = self.root / "plugins/p/src"
        path.mkdir(parents=True)
        (path / "old.rs").write_text("code")
        base = self.commit()
        target = self.root / "apps/web"
        target.mkdir(parents=True)
        (path / "old.rs").rename(target / "new.ts")
        self.commit()
        result = plan.plan(self.root, {}, base)
        self.assertFalse(result["fallback"])
        self.assertTrue(all(result["families"].values()))


class CoverageTests(Repository):
    def fixture(self):
        (self.root / "covered.rs").write_text("first\nsecond\n")
        (self.root / "missing.ts").write_text("old\n")
        (self.root / "deleted.rs").write_text("removed\n")
        base = self.commit()
        (self.root / "covered.rs").write_text("first\nchanged\n// unmapped\n")
        (self.root / "missing.ts").write_text("new\n")
        (self.root / "deleted.rs").unlink()
        self.commit()
        report = Path(self.directory.name) / "coverage.lcov"
        report.write_text("SF:covered.rs\nDA:1,2\nDA:2,0\nend_of_record\n")
        return base, report

    def test_diff_reports_uncovered_missing_unmapped_and_deletions(self):
        base, lcov = self.fixture()
        result = line_coverage.report(self.root, [lcov], base)
        self.assertEqual(result["diff"]["executable"], 1)
        self.assertEqual(result["diff"]["percent"], 0)
        self.assertEqual(result["diff"]["uncovered_lines"]["covered.rs"], [2])
        self.assertEqual(result["diff"]["unmeasured_files"], ["missing.ts"])
        self.assertEqual(result["diff"]["unmapped_added_lines"]["covered.rs"], [3])
        self.assertEqual(result["diff"]["deleted_paths_excluded"], ["deleted.rs"])
        self.assertFalse(result["policy"]["baseline_established"])

    def test_missing_history_is_unavailable_never_full_coverage(self):
        _, lcov = self.fixture()
        result = line_coverage.report(self.root, [lcov], "absent")
        self.assertFalse(result["diff"]["available"])
        self.assertIsNone(result["diff"]["percent"])

    def test_renamed_destination_is_measured_as_added_source(self):
        (self.root / "before.rs").write_text("line\n")
        base = self.commit()
        (self.root / "before.rs").rename(self.root / "after.rs")
        self.commit()
        lcov = Path(self.directory.name) / "report.lcov"
        lcov.write_text("SF:after.rs\nDA:1,1\nend_of_record\n")
        result = line_coverage.report(self.root, [lcov], base)
        self.assertEqual(result["diff"]["covered"], 1)
        self.assertEqual(result["diff"]["deleted_paths_excluded"], ["before.rs"])

    def test_invalid_counts_are_rejected_and_duplicate_records_merge(self):
        _, lcov = self.fixture()
        lcov.write_text("SF:covered.rs\nDA:1,-1\nend_of_record\n")
        with self.assertRaises(ValueError):
            line_coverage.read_lcov(self.root, [lcov])
        lcov.write_text("SF:covered.rs\nDA:1,0\nend_of_record\nSF:covered.rs\nDA:1,2\nend_of_record\n")
        self.assertEqual(line_coverage.read_lcov(self.root, [lcov])[0]["covered.rs"][1], 2)


class ArchitectureTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        # Exercise the real public Cargo resolver once; mutations below operate on
        # its actual package IDs, feature resolution and dependency edge shape.
        cls.metadata = architecture.load_metadata(ROOT)

    def mutated(self, source, destination, kind=None, target=None):
        metadata = copy.deepcopy(self.metadata)
        packages = {p["name"]: p["id"] for p in metadata["packages"]}
        node = next(n for n in metadata["resolve"]["nodes"] if n["id"] == packages[source])
        node["deps"].append({"name": destination.replace("-", "_"), "pkg": packages[destination],
                             "dep_kinds": [{"kind": kind, "target": target}]})
        return metadata

    def test_real_workspace_passes(self):
        self.assertEqual(architecture.check(ROOT, self.metadata)["errors"], [])

    def test_contract_cannot_import_provider_transitively(self):
        metadata = self.mutated("swarmie-agent-contracts", "swarmie-model-provider-genai")
        errors = architecture.check(ROOT, metadata)["errors"]
        self.assertTrue(any("portable Contracts" in item for item in errors))

    def test_target_build_edge_cannot_bypass_agent_policy(self):
        metadata = self.mutated("swarmie-agent-loop-default", "swarmie-task-storage-sqlite", "build", 'cfg(target_os = "windows")')
        self.assertTrue(any("Agent loop imports" in item for item in architecture.check(ROOT, metadata)["errors"]))

    def test_development_fixture_dependency_is_allowed(self):
        metadata = self.mutated("swarmie-agent-loop-default", "swarmie-task-storage-sqlite", "dev")
        self.assertEqual(architecture.check(ROOT, metadata)["errors"], [])

    def test_scheduler_cannot_import_sqlite_and_original_graph_recovers(self):
        metadata = self.mutated("swarmie-durable-tasks", "rusqlite")
        self.assertTrue(any("scheduler depends" in item for item in architecture.check(ROOT, metadata)["errors"]))
        self.assertEqual(architecture.check(ROOT, self.metadata)["errors"], [])


class LocalTests(unittest.TestCase):
    def test_contract_change_selects_actual_transitive_consumers(self):
        metadata = architecture.load_metadata(ROOT)
        with patch.object(local, "load_metadata", return_value=metadata), patch.object(local, "changed_paths", return_value=["crates/model-contracts/src/lib.rs"]), patch.object(local, "git", return_value=b""):
            result = local.feedback(ROOT, "HEAD")
        self.assertIn("swarmie-agent-loop-default", result["affected_rust_packages"])
        self.assertIn("swarmie-model-provider-genai", result["affected_rust_packages"])
        self.assertIn("swarmie-runtime", result["affected_rust_packages"])
        self.assertNotIn("swarmie-memory-store", result["affected_rust_packages"])
        self.assertFalse(result["families"]["frontend"])

    def test_missing_critical_case_is_not_presumed_passed(self):
        manifest = {"criteria": {"criterion": {"success": ["missing"]}}}
        self.assertEqual(cases.trace(manifest, {})["criterion"]["success"]["missing"], "not_executed")


class RuntimeTraceTests(unittest.TestCase):
    def fixture(self, log):
        source = "tests/plugins/fixtures/agent-loop/tests/runtime.rs"
        name = "functional_tool_turn_runs_multiple_steps_and_enforces_final_policy"
        manifest = {"scope": "fixture", "limitations": [], "criteria": {"effect": {"success": {"source": source, "test": name}}}}
        receipt = {"log_sha256": hashlib.sha256(log.encode()).hexdigest(), "source_changed": False,
                   "source_before": {"commit": git(ROOT, "rev-parse", "HEAD").decode().strip(),
                                     "files": {source: hashlib.sha256((ROOT / source).read_bytes()).hexdigest()}},
                   "argv": ["cargo", "test"], "exit_code": 0}
        return manifest, receipt

    def test_named_runtime_result_is_bound_to_source_and_receipt(self):
        log = "test functional_tool_turn_runs_multiple_steps_and_enforces_final_policy ... ok\n"
        manifest, receipt = self.fixture(log)
        result = runtime_trace.trace(ROOT, manifest, log, receipt)
        self.assertEqual(result["criteria"]["effect"]["success"]["outcome"], "passed")
        self.assertEqual(result["acceptance"], "not_determined")
        receipt["log_sha256"] = "wrong"
        with self.assertRaises(ValueError):
            runtime_trace.trace(ROOT, manifest, log, receipt)

    def test_missing_duplicate_skipped_and_changed_source_are_explicit(self):
        name = "functional_tool_turn_runs_multiple_steps_and_enforces_final_policy"
        for log, outcome in (("", "not_executed"), (f"test {name} ... ok\ntest {name} ... ok\n", "ambiguous"),
                             (f"test {name} ... ignored\n", "skipped")):
            manifest, receipt = self.fixture(log)
            self.assertEqual(runtime_trace.trace(ROOT, manifest, log, receipt)["criteria"]["effect"]["success"]["outcome"], outcome)
        log = f"test {name} ... ok\n"
        manifest, receipt = self.fixture(log)
        receipt["source_before"]["files"] = {}
        self.assertEqual(runtime_trace.trace(ROOT, manifest, log, receipt)["criteria"]["effect"]["success"]["outcome"], "source_unverified")


class EvidenceTests(Repository):
    def setUp(self):
        super().setUp()
        (self.root / "input.rs").write_text("input")
        self.commit()

    def test_failure_is_retained_then_success_is_independently_recorded(self):
        for name, exit_code in (("rejected", 7), ("recovered", 0)):
            output = Path(self.directory.name) / name
            code = evidence.run(self.root, output, [sys.executable, "-c", f"print('evidence'); raise SystemExit({exit_code})"], ["quality.evidence.exit-status"])
            self.assertEqual(code, exit_code)
            result = json.loads((output / "result.json").read_text())
            self.assertEqual(result["exit_code"], exit_code)
            self.assertEqual(result["acceptance"], "not_determined")
            self.assertFalse(result["source_changed"])
            self.assertTrue(result["log_sha256"])

    def test_coverage_receipt_rejects_stale_source(self):
        (self.root / ".gitignore").write_text("target/\n")
        self.commit()
        output = Path(self.directory.name) / "producer"
        command = [sys.executable, "-c", "from pathlib import Path; p=Path('target/report.lcov'); p.parent.mkdir(); p.write_text('SF:input.rs\\nDA:1,1\\nend_of_record\\n')"]
        self.assertEqual(evidence.run(self.root, output, command, artifacts=["target/*.lcov"]), 0)
        lcov = self.root / "target/report.lcov"
        result = line_coverage.report(self.root, [lcov], "HEAD", output / "result.json")
        self.assertTrue(result["producer_identity_verified"])
        (self.root / "input.rs").write_text("new source")
        with self.assertRaises(ValueError):
            line_coverage.report(self.root, [lcov], "HEAD", output / "result.json")

    def test_output_reuse_and_output_inside_source_are_rejected(self):
        for output in (self.root / "reports", Path(self.directory.name)):
            with self.assertRaises(ValueError):
                evidence.run(self.root, output, [sys.executable, "-c", "pass"])

    def test_source_change_and_missing_executable_are_explicit(self):
        output = Path(self.directory.name) / "changed"
        evidence.run(self.root, output, [sys.executable, "-c", "open('input.rs','w').write('changed')"])
        self.assertTrue(json.loads((output / "result.json").read_text())["source_changed"])
        output = Path(self.directory.name) / "missing"
        self.assertEqual(evidence.run(self.root, output, ["quality-missing-executable"]), 127)
        self.assertEqual(json.loads((output / "result.json").read_text())["outcome"], "failed")


if __name__ == "__main__":
    unittest.main()
