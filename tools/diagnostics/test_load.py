"""Load campaign bounds, retained failures, and honest store readback."""

import json
import io
import os
from contextlib import redirect_stderr
from pathlib import Path
import sqlite3
import tempfile
import unittest
from unittest.mock import patch

from common import DiagnosticError, SOURCE_IDENTITY_KEYS
from load import command, execute, parser, resource_slot, store_configuration


class LoadTests(unittest.TestCase):
    def test_invalid_bounds_are_refused_before_build(self):
        for flag, value in (("--turns", "1025"), ("--concurrency", "129"),
                            ("--max-parallel", "0"), ("--arrival-interval-ms", "0")):
            with redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
                parser().parse_args(["--output", "/tmp/fresh", flag, value])

    def test_command_preserves_spaces_and_scheduler_configuration(self):
        args = parser().parse_args(["--output", "/tmp/fresh", "--max-parallel", "16"])
        argv = command(Path("/tmp/a binary"), Path("/tmp/a result"), args, 32, 20)
        self.assertEqual(argv[0], "/tmp/a binary")
        self.assertEqual(argv[2], "/tmp/a result")
        self.assertEqual(argv[argv.index("--concurrency")+1], "32")
        self.assertEqual(argv[argv.index("--max-parallel")+1], "16")

    def test_failed_level_is_retained_and_campaign_continues(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            binary = output / "binary"
            binary.write_bytes(b"test binary")
            args = parser().parse_args(["--output", str(output), "--concurrency", "1", "2"])
            def run(argv, _output, label, _timeout):
                destination = Path(argv[2])
                destination.mkdir()
                (destination / "summary.json").write_text(json.dumps({"valid": label != "level-001", "statistics": {"outlier": 999}}))
                if label == "level-001":
                    raise DiagnosticError("retained failure")
            with patch("load.provenance", return_value={key: None for key in SOURCE_IDENTITY_KEYS}), patch("load.build", return_value=binary), patch("load.run_command", side_effect=run), patch("load.verify_capture"):
                result = execute(args, output)
            self.assertTrue(result["complete"])
            self.assertFalse(result["valid"])
            self.assertEqual(len(result["levels"]), 2)
            self.assertEqual(result["levels"][0]["command_error"], "retained failure")
            self.assertEqual(result["levels"][0]["statistics"]["outlier"], 999)
            self.assertTrue(result["levels"][1]["valid"])

    def test_campaign_budget_stops_before_starting_another_child(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            binary = output / "binary"
            binary.write_bytes(b"test binary")
            args = parser().parse_args(["--output", str(output), "--concurrency", "1", "2", "--campaign-timeout", "1"])
            with patch("load.provenance", return_value={key: None for key in SOURCE_IDENTITY_KEYS}), patch("load.build", return_value=binary), patch("load.time.monotonic", side_effect=[0, 2]), patch("load.run_command") as run:
                result = execute(args, output)
            run.assert_not_called()
            self.assertFalse(result["complete"])
            self.assertEqual(result["unstarted_levels"], 2)
            self.assertEqual(result["stop_reason"], "campaign_budget_exhausted")

    @unittest.skipUnless(os.name == "posix", "POSIX advisory locks")
    def test_resource_slot_refuses_overlap_without_running_work(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "resource.lock"
            with resource_slot(path):
                with self.assertRaises(DiagnosticError):
                    with resource_slot(path):
                        self.fail("overlapping slot acquired")
            with resource_slot(path):
                pass

    def test_new_connection_does_not_claim_writer_synchronous(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            with sqlite3.connect(output / "tasks.sqlite") as db:
                db.execute("PRAGMA journal_mode=WAL")
                db.execute("PRAGMA synchronous=FULL")
            actual = store_configuration(output)
            self.assertEqual(actual["tasks"]["journal_mode"], "wal")
            self.assertIsNone(actual["tasks"]["writer_synchronous"])


if __name__ == "__main__":
    unittest.main()
