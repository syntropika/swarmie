"""Regression checks for source attribution, fresh outputs and owned signal cleanup."""

import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time
import unittest

from common import ROOT, git
import evidence
import line_coverage
import runtime_trace
from test_quality import Repository


class RuntimeProvenanceTests(Repository):
    def setUp(self):
        super().setUp()
        (self.root / "scenario.rs").write_text("fn observed_case() {}\n")
        (self.root / "implementation.rs").write_text("original\n")
        self.commit()
        self.log = "test observed_case ... ok\n"
        self.manifest = {"scope": "quality fixture", "limitations": [],
                         "criteria": {"fixture": {"success": {"source": "scenario.rs", "test": "observed_case"}}}}

    def receipt(self, name):
        output = Path(self.directory.name) / name
        self.assertEqual(evidence.run(self.root, output, [sys.executable, "-c", f"print({self.log!r}, end='')"]), 0)
        return json.loads((output / "result.json").read_text())

    def test_dirty_implementation_is_rejected_then_clean_commit_is_attributed(self):
        (self.root / "implementation.rs").write_text("changed without a commit\n")
        dirty = self.receipt("dirty")
        self.assertTrue(dirty["source_before"]["dirty"])
        self.assertFalse(dirty["source_changed"])
        with self.assertRaises(ValueError):
            runtime_trace.trace(self.root, self.manifest, self.log, dirty)
        revision = self.commit()
        clean = self.receipt("clean")
        result = runtime_trace.trace(self.root, self.manifest, self.log, clean)
        self.assertEqual(result["source_commit"], revision)
        self.assertEqual(result["source_identity"]["files_sha256"], clean["source_before"]["files_sha256"])
        self.assertEqual(result["criteria"]["fixture"]["success"]["outcome"], "passed")

    def test_git_clean_flag_cannot_override_changed_committed_implementation_bytes(self):
        git(self.root, "update-index", "--assume-unchanged", "implementation.rs")
        (self.root / "implementation.rs").write_text("changed behind a clean Git status\n")
        receipt = self.receipt("hidden-change")
        self.assertFalse(receipt["source_before"]["dirty"])
        with self.assertRaises(ValueError):
            runtime_trace.trace(self.root, self.manifest, self.log, receipt)

    def test_interrupted_receipt_with_zero_command_exit_cannot_qualify(self):
        receipt = self.receipt("incomplete")
        receipt.update({"outcome": "interrupted", "runner_exit_code": 143, "termination_signal": 15})
        self.assertEqual(receipt["exit_code"], 0)
        with self.assertRaises(ValueError):
            runtime_trace.trace(self.root, self.manifest, self.log, receipt)

    def test_historical_receipt_without_snapshot_does_not_qualify_a_commit(self):
        revision = git(self.root, "rev-parse", "HEAD").decode().strip()
        receipt = {"steps": [{"log_sha256": hashlib.sha256(self.log.encode()).hexdigest(),
                              "commit": revision, "exit_code": 0, "argv": ["cargo", "test"]}]}
        result = runtime_trace.trace(self.root, self.manifest, self.log, receipt)
        self.assertIsNone(result["source_commit"])
        self.assertEqual(result["reported_source_commit"], revision)
        self.assertEqual(result["criteria"]["fixture"]["success"]["outcome"], "source_unverified")


class ProducerProvenanceTests(Repository):
    def setUp(self):
        super().setUp()
        (self.root / ".gitignore").write_text("target/\n")
        (self.root / "source.rs").write_text("original\n")
        self.base = self.commit()
        self.stale = self.root / "target/stale.lcov"
        self.stale.parent.mkdir()
        self.payload = "SF:source.rs\nDA:1,1\nend_of_record\n"
        self.stale.write_text(self.payload)

    def test_noop_cannot_rebind_a_stale_captured_report_to_new_source(self):
        (self.root / "source.rs").write_text("changed\n")
        self.commit()
        output = Path(self.directory.name) / "noop"
        self.assertEqual(evidence.run(self.root, output, [sys.executable, "-c", "pass"],
                                      artifacts=["target/stale.lcov"]), 0)
        with self.assertRaises(ValueError):
            line_coverage.report(self.root, [self.stale], self.base, [output / "result.json"])
        self.assertEqual(self.stale.read_text(), self.payload)

    def test_captured_input_paths_cannot_escape_into_owned_outputs(self):
        output = Path(self.directory.name) / "unsafe-capture"
        with self.assertRaises(ValueError):
            evidence.run(self.root, output, [sys.executable, "-c", "pass"],
                         artifacts=["../produced/coverage.lcov"], produces=["coverage.lcov"])
        self.assertFalse(output.exists())
        self.assertEqual(self.stale.read_text(), self.payload)

    def test_identical_fresh_reports_qualify_without_removing_old_user_artifacts(self):
        for name in ("first", "second"):
            output = Path(self.directory.name) / name
            command = [sys.executable, "-c", f"from pathlib import Path; Path('{{quality_output}}/coverage.lcov').write_text({self.payload!r})"]
            self.assertEqual(evidence.run(self.root, output, command, produces=["coverage.lcov"]), 0)
            result = line_coverage.report(self.root, [output / "produced/coverage.lcov"], self.base,
                                          [output / "result.json"])
            self.assertTrue(result["producer_identity_verified"])
        self.assertEqual(self.stale.read_text(), self.payload)

    def test_missing_declared_output_retains_actual_exit_and_fails_runner(self):
        output = Path(self.directory.name) / "missing"
        self.assertNotEqual(evidence.run(self.root, output, [sys.executable, "-c", "pass"],
                                         produces=["coverage.lcov"]), 0)
        receipt = json.loads((output / "result.json").read_text())
        self.assertEqual(receipt["exit_code"], 0)
        self.assertNotEqual(receipt["outcome"], "passed")
        self.assertEqual(receipt["produced_artifacts"], {})
        with self.assertRaises(ValueError):
            line_coverage.report(self.root, [self.stale], self.base, [output / "result.json"])

    def test_symlink_and_hardlink_outputs_cannot_claim_fresh_provenance(self):
        for operation in ("symlink_to", "hardlink_to"):
            output = Path(self.directory.name) / operation
            command = [sys.executable, "-c", f"from pathlib import Path; Path('{{quality_output}}/coverage.lcov').{operation}({str(self.stale)!r})"]
            self.assertNotEqual(evidence.run(self.root, output, command, produces=["coverage.lcov"]), 0)
            receipt = json.loads((output / "result.json").read_text())
            self.assertEqual(receipt["produced_artifacts"], {})
        self.assertEqual(self.stale.read_text(), self.payload)


@unittest.skipUnless(os.name == "posix", "owned process groups require POSIX")
class SignalSettlementTests(Repository):
    def setUp(self):
        super().setUp()
        (self.root / "source.rs").write_text("source\n")
        self.commit()

    def interrupted(self, stubborn=False):
        directory = Path(self.directory.name)
        ready = directory / "ready.json"
        output = directory / "execution"
        child = f'''import json, os, signal, subprocess, sys, time
stop = False
def terminate(*_):
    global stop
    stop = True
signal.signal(signal.SIGTERM, signal.SIG_IGN if {stubborn!r} else terminate)
grandchild = None if {stubborn!r} else subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(30)'])
with open({str(ready)!r}, 'w') as stream:
    json.dump({{'pid': os.getpid(), 'group': os.getpgrp(), 'grandchild': grandchild.pid if grandchild else None}}, stream)
while not stop:
    time.sleep(0.02)
if grandchild:
    grandchild.wait(timeout=3)
'''
        driver = f"import sys; from pathlib import Path; sys.path.insert(0, {str(ROOT / 'tools/quality')!r}); import evidence; raise SystemExit(evidence.run(Path({str(self.root)!r}), Path({str(output)!r}), [sys.executable, '-c', {child!r}]))"
        wrapper = subprocess.Popen([sys.executable, "-c", driver], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        unrelated = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(30)"])
        group, settled = None, False
        try:
            deadline = time.monotonic() + 10
            while not ready.exists() and wrapper.poll() is None and time.monotonic() < deadline:
                time.sleep(0.02)
            self.assertTrue(ready.exists(), "child readiness was not observed")
            # The child writes a small complete JSON before the parent sends SIGTERM.
            while True:
                try:
                    group = json.loads(ready.read_text())["group"]
                    break
                except json.JSONDecodeError:
                    if time.monotonic() >= deadline:
                        raise
                    time.sleep(0.01)
            wrapper.terminate()
            stdout, stderr = wrapper.communicate(timeout=12)
            self.assertTrue((output / "result.json").exists(), (stdout, stderr))
            receipt = json.loads((output / "result.json").read_text())
            self.assertEqual(wrapper.returncode, 128 + signal.SIGTERM)
            self.assertEqual(receipt["termination_signal"], signal.SIGTERM)
            self.assertNotEqual(receipt["outcome"], "passed")
            self.assertTrue(receipt["cleanup"]["group_settled"])
            with self.assertRaises(ProcessLookupError):
                os.killpg(group, 0)
            settled = True
            self.assertIsNone(unrelated.poll(), "unrelated process must not receive group signals")
            if stubborn:
                self.assertTrue(receipt["cleanup"]["kill_sent"])
                self.assertEqual(receipt["exit_code"], -signal.SIGKILL)
            else:
                self.assertEqual(receipt["exit_code"], 0)
            self.assertTrue((output / "command.log").exists())
            self.assertFalse(receipt["source_changed"])
        finally:
            if group and not settled:
                try:
                    os.killpg(group, signal.SIGKILL)
                except ProcessLookupError:
                    pass
            if wrapper.poll() is None:
                wrapper.kill()
                wrapper.communicate(timeout=5)
            if unrelated.poll() is None:
                unrelated.terminate()
            unrelated.wait(timeout=5)

    def test_timeout_retains_actual_child_status_after_owned_cleanup(self):
        output = Path(self.directory.name) / "timeout"
        code = evidence.run(self.root, output, [sys.executable, "-c", "import time; time.sleep(30)"], timeout=0.1)
        receipt = json.loads((output / "result.json").read_text())
        self.assertEqual(code, 124)
        self.assertEqual(receipt["exit_code"], -signal.SIGTERM)
        self.assertEqual(receipt["runner_exit_code"], 124)
        self.assertTrue(receipt["cleanup"]["group_settled"])
        self.assertIsNone(receipt["termination_signal"])

    def test_sigterm_settles_owned_child_and_grandchild_and_retains_receipt(self):
        self.interrupted()

    def test_sigterm_escalates_for_an_owned_child_that_ignores_termination(self):
        self.interrupted(stubborn=True)
