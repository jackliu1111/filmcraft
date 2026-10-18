"""Exercise the public packaging command; never build or copy model weights here."""
from pathlib import Path
import os
import json
import subprocess
import sys
import tempfile
import unittest

COMMAND = Path(__file__).resolve().parents[1] / "build_runtime.py"


class PackagingCommandTests(unittest.TestCase):
    def command(self, root, *extra):
        return subprocess.run([
            sys.executable, str(COMMAND), "--backend", str(root / "backend"),
            "--model-workspace", str(root / "model"), "--ffmpeg", str(root / "ffmpeg"),
            "--ffprobe", str(root / "ffprobe"), "--media-license-dir", str(root / "licenses"),
            "--output", str(root / "runtime"), "--work-dir", str(root / "work"), *extra,
        ], capture_output=True, text=True)

    def test_local_output_is_rejected_without_creating_any_artifact(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            result = self.command(root, "--check-only")
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("mounted external", result.stderr)
            self.assertEqual(list(root.iterdir()), [])

    def test_existing_output_is_never_overwritten(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "runtime").mkdir()
            marker = root / "runtime" / "original.txt"
            marker.write_text("keep")
            result = self.command(root)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("refusing to overwrite", result.stderr)
            self.assertEqual(marker.read_text(), "keep")

    @unittest.skipUnless(os.environ.get("FILMCRAFT_AI_TEST_VOLUME"), "requires external APFS test volume")
    def test_incomplete_model_cannot_start_a_build_or_create_output(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            output = Path(os.environ["FILMCRAFT_AI_TEST_VOLUME"]) / (root.name + "-runtime")
            work = output.with_name(output.name + "-work")
            result = self.command(root, "--check-only", "--output", str(output), "--work-dir", str(work))
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("BEATs_strong_1.pt", result.stderr)
            self.assertFalse(output.exists())
            self.assertFalse(work.exists())

    @unittest.skipUnless(os.environ.get("FILMCRAFT_AI_BUILD_INPUTS"), "requires native inference build dependencies")
    def test_check_only_reports_contract_and_dependency_plan_without_writes(self):
        inputs = json.loads(os.environ["FILMCRAFT_AI_BUILD_INPUTS"])
        command = [sys.executable, str(COMMAND), "--check-only"]
        for key, value in inputs.items():
            command.extend(["--" + key, value])
        result = subprocess.run(command, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        plan = json.loads(result.stdout)
        self.assertEqual(plan["manifest"]["service"], "service/filmcraft-ai")
        self.assertEqual(plan["manifest"]["checkpoint"], "model/checkpoints/BEATs_strong_1.pt")
        self.assertEqual(plan["packages"]["pyinstaller"], "6.16.0")
        self.assertEqual(plan["backend_commit"], "978fed1580fdf85d34e2b5c17f973693c5510693")
        self.assertGreater(plan["minimum_free_bytes"], 1073741824)
        self.assertFalse(Path(inputs["output"]).exists())
        self.assertFalse(Path(inputs["work-dir"]).exists())


if __name__ == "__main__":
    unittest.main()
