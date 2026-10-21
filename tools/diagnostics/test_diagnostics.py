"""Behavior checks for safety, failures, discovery, and stack attribution."""

import json
from contextlib import redirect_stderr
import io
import os
from pathlib import Path
import subprocess
import signal
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

from common import DiagnosticError, cargo_command, discover_binary, fresh_output, repository_hashes, run_command, sha256, terminate_inferior
from diagnose import benchmark_summary, execute, gdb_command, parser, valid_summary, verify_capture, workload_command
from stacks import aggregate


class DiagnosticTests(unittest.TestCase):
    def test_output_rejects_checkout_and_existing_user_directory(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            root = base / "checkout"
            root.mkdir()
            with self.assertRaises(DiagnosticError):
                fresh_output(root / "raw", root)
            outside = fresh_output(base / "raw", root)
            (outside / "keep").write_text("user data")
            with self.assertRaises(DiagnosticError):
                fresh_output(outside, root)
            self.assertEqual((outside / "keep").read_text(), "user data")

    def test_symlink_cannot_redirect_output_inside_checkout(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "checkout"
            root.mkdir()
            alias = Path(directory) / "alias"
            alias.symlink_to(root, target_is_directory=True)
            with self.assertRaises(DiagnosticError):
                fresh_output(alias / "raw", root)

    def test_cargo_discovers_only_exact_example_artifact(self):
        unrelated = {"reason": "compiler-artifact", "target": {"name": "other", "kind": ["example"]}, "executable": "/tmp/other"}
        expected = {"reason": "compiler-artifact", "target": {"name": "runtime-diagnostic", "kind": ["example"]}, "executable": "/tmp/a binary"}
        text = "not json\n" + json.dumps(unrelated) + "\n" + json.dumps(expected)
        self.assertEqual(discover_binary(text), Path("/tmp/a binary"))
        expected["executable"] = "/tmp/second"
        with self.assertRaises(DiagnosticError):
            discover_binary(text + "\n" + json.dumps(expected))
        with self.assertRaises(DiagnosticError):
            discover_binary(json.dumps(unrelated))

    def test_commands_preserve_paths_and_explicit_build_profile(self):
        argv = workload_command(Path("/tmp/a binary"), Path("/tmp/output with spaces"), 2, "wait", 30)
        self.assertEqual(argv[0], "/tmp/a binary")
        self.assertEqual(argv[2], "/tmp/output with spaces")
        cargo = cargo_command("dev")
        self.assertIn("--locked", cargo)
        self.assertTrue(cargo[1].startswith("+"))
        self.assertEqual(cargo[cargo.index("--profile") + 1], "dev")
        self.assertEqual(gdb_command(Path(argv[0]), argv)[-len(argv):], argv)

    def test_nonzero_command_is_retained_and_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            with self.assertRaises(DiagnosticError):
                run_command([sys.executable, "-c", "print('retained'); raise SystemExit(7)"], output, "case", 2)
            self.assertEqual(json.loads((output / "case.command.json").read_text())["exit_code"], 7)
            self.assertIn("retained", (output / "case.stdout.log").read_text())

    def test_timeout_is_retained_and_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            with self.assertRaises(DiagnosticError):
                run_command([sys.executable, "-c", "import time; time.sleep(10)"], output, "case", 0.05)
            record = json.loads((output / "case.command.json").read_text())
            self.assertTrue(record["timed_out"])
            self.assertLess(record["exit_code"], 0)

    @unittest.skipUnless(Path("/proc/self/stat").exists(), "Linux process identity check")
    def test_debugger_cleanup_checks_parent_and_process_start_identity(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            child = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(10)"])
            try:
                fields = Path(f"/proc/{child.pid}/stat").read_text().rsplit(") ", 1)[1].split()
                (output / "inferior.json").write_text(json.dumps({"pid": child.pid, "start_ticks": "wrong"}))
                terminate_inferior(output, os.getpid())
                self.assertIsNone(child.poll())
                (output / "inferior.json").write_text(json.dumps({"pid": child.pid, "start_ticks": fields[19]}))
                terminate_inferior(output, os.getpid())
                self.assertLess(child.wait(timeout=2), 0)
            finally:
                if child.poll() is None:
                    child.kill()
                    child.wait()

    def test_partial_inferior_record_does_not_prevent_watchdog_cleanup(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            (output / "inferior.json").write_text("{")
            with self.assertRaises(DiagnosticError):
                run_command([sys.executable, "-c", "import time; time.sleep(10)"], output, "gdb", 0.05)
            record = json.loads((output / "gdb.command.json").read_text())
            self.assertTrue(record["timed_out"])
            self.assertTrue(record["cleanup_error"])

    @unittest.skipUnless(os.name == "posix", "POSIX interrupt and process groups")
    def test_interrupt_reaps_owned_child_and_retains_receipt(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            pid_path = output / "child.pid"
            child_code = f"import os,time;from pathlib import Path;Path({str(pid_path)!r}).write_text(str(os.getpid()));time.sleep(10)"
            helper_code = f"from pathlib import Path;from common import run_command;run_command({[sys.executable, '-c', child_code]!r},Path({directory!r}),'interrupt',20)"
            env = dict(os.environ, PYTHONPATH=str(Path(__file__).resolve().parent))
            helper = subprocess.Popen([sys.executable, "-c", helper_code], env=env,
                                      stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            child_pid = None
            try:
                deadline = time.monotonic() + 3
                while not pid_path.exists() and time.monotonic() < deadline:
                    time.sleep(0.01)
                self.assertTrue(pid_path.exists(), "child never became ready")
                child_pid = int(pid_path.read_text())
                helper.send_signal(signal.SIGINT)
                self.assertNotEqual(helper.wait(timeout=3), 0)
                record = json.loads((output / "interrupt.command.json").read_text())
                self.assertTrue(record["interrupted"])
                self.assertFalse(record["timed_out"])
                with self.assertRaises(ProcessLookupError):
                    os.kill(child_pid, 0)
                child_pid = None
            finally:
                if child_pid is not None:
                    try:
                        os.kill(child_pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass
                if helper.poll() is None:
                    helper.kill()
                    helper.wait()

    def test_invalid_or_partial_workload_cannot_be_success(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)
            data = {"schema_version": 1, "valid": True, "runs": [{"valid": True, "error": None}]}
            (path / "summary.json").write_text(json.dumps(data))
            valid_summary(path, 1)
            with self.assertRaises(DiagnosticError):
                valid_summary(path, 2)
            data["runs"][0]["valid"] = False
            (path / "summary.json").write_text(json.dumps(data))
            with self.assertRaises(DiagnosticError):
                valid_summary(path, 1)

    def test_recursive_inclusive_presence_is_counted_once_per_sample(self):
        report = aggregate([
            {"frames": [{"name": "recursive"}, {"name": "recursive"}, {"name": "parent"}], "truncated": True},
            {"frames": [{"name": "wait"}, {"name": "parent"}]},
            {"frames": [{"name": None}], "unwind_error": "cannot unwind"},
        ])
        counts = {row["function"]: row["samples"] for row in report["inclusive"]}
        self.assertEqual(counts["recursive"], 1)
        self.assertEqual(counts["parent"], 2)
        self.assertEqual(report["unresolved_samples"], 1)
        self.assertEqual(report["truncated_samples"], 1)
        self.assertEqual(report["unwind_error_samples"], 1)
        self.assertEqual(report["top_frame"][0]["samples"], 1)
        self.assertNotIn("cpu_percentage", report)

    def test_unsupported_cpu_clock_remains_unavailable(self):
        summary = benchmark_summary({"observer": "wait", "runs": [{"window": {"wall_ns": 2000000000, "thread_cpu_ns": None}}]})
        self.assertEqual(summary["median_wall_seconds"], 2)
        self.assertIsNone(summary["median_thread_cpu_seconds"])

    def test_capture_rejects_replaced_binary_or_changed_source(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            binary = output / "binary"
            binary.write_bytes(b"first executable")
            digest = sha256(binary)
            expected = {"profile": "dev", "head": "commit", "status": "dirty", "tracked_diff_sha256": "diff", "repository_file_sha256": {"source": "hash"}}
            with patch("diagnose.provenance", return_value=expected):
                verify_capture(binary, digest, expected, output, "reference")
                binary.write_bytes(b"replaced executable")
                with self.assertRaisesRegex(DiagnosticError, "binary changed"):
                    verify_capture(binary, digest, expected, output, "reference")
            binary.write_bytes(b"first executable")
            with patch("diagnose.provenance", return_value=dict(expected, tracked_diff_sha256="changed")):
                with self.assertRaisesRegex(DiagnosticError, "source inputs changed"):
                    verify_capture(binary, digest, expected, output, "reference")

    def test_new_production_input_mutated_during_build_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "source"
            root.mkdir()
            subprocess.run(["git", "init", "-q", str(root)], check=True)
            (root / ".gitignore").write_text("ignored-local-secret\n")
            production = root / "production.rs"
            production.write_text("mod generated;\n")
            subprocess.run(["git", "add", "."], cwd=root, check=True)
            untracked = root / "generated.rs"
            untracked.write_text("const VALUE:u8=1;\n")
            (root / "ignored-local-secret").write_text("must not be recorded")
            first = repository_hashes(root)
            self.assertIn("production.rs", first)
            self.assertIn("generated.rs", first)
            self.assertNotIn("ignored-local-secret", first)
            output = Path(directory) / "output"
            output.mkdir()
            binary = output / "binary"
            binary.write_bytes(b"executable")
            def snapshot(profile):
                return {"profile": profile, "head": "commit", "status": "same dirty status",
                        "tracked_diff_sha256": "same tracked patch", "repository_file_sha256": repository_hashes(root)}
            args = parser().parse_args(["bench", "--output", str(output)])
            real_check_output = subprocess.check_output
            def check_output(argv, **kwargs):
                return b"" if argv[1] == "diff" else real_check_output(argv, **kwargs)
            for changed in (production, untracked):
                with self.subTest(input=changed.name):
                    def mutate_during_build(*args):
                        changed.write_text(changed.read_text() + "// changed during build\n")
                        return binary
                    with patch("diagnose.provenance", side_effect=snapshot), patch("diagnose.build", side_effect=mutate_during_build), patch("diagnose.subprocess.check_output", side_effect=check_output), patch("diagnose.run_command") as run:
                        with self.assertRaisesRegex(DiagnosticError, "changed during build"):
                            execute(args, output)
                        run.assert_not_called()

    def test_positive_bounds_and_fixed_observer_choices(self):
        with self.assertRaises(SystemExit), redirect_stderr(io.StringIO()):
            parser().parse_args(["profile", "--output", "/tmp/raw", "--max-samples", "0"])
        self.assertEqual(parser().parse_args(["bench", "--output", "/tmp/raw"]).observer, "wait")


if __name__ == "__main__":
    unittest.main()
