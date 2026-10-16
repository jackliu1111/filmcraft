"""Verify the relocated entry against independently configured backend checkouts."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[3]
ENTRY = ROOT / 'integrations/apex-ai/bridge.py'


class BridgeEntryTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.folder = Path(self.temp.name)
        self.backend = self.folder / 'backend 有空格'
        (self.backend / 'integrations/filmcraft').mkdir(parents=True)
        self.cwd = self.folder / 'unrelated'
        self.cwd.mkdir()

    def tearDown(self):
        self.temp.cleanup()

    def run_entry(self, backend=None):
        env = dict(os.environ)
        env.pop('APEX_AI_DATA_DIR', None)
        env['APEX_SERVICE_ROOT'] = str(backend or self.backend)
        return subprocess.run([sys.executable, str(ENTRY), 'probe', '--input', '/样本/片段.mkv'],
                              cwd=self.cwd, env=env, text=True, capture_output=True, timeout=10)

    def write_backend(self, text):
        (self.backend / 'integrations/filmcraft/bridge.py').write_text(text)

    def test_explicit_backend_from_unrelated_cwd_preserves_protocol(self):
        (self.backend / 'backend_helper.py').write_text("IDENTITY = 'configured-backend'\n")
        self.write_backend(
            "import json\nfrom backend_helper import IDENTITY\nprint('backend import log')\n"
            "def main(argv, *, env):\n"
            " print(json.dumps({'type':'media','backend':IDENTITY,'args':argv,'data_dir':env['APEX_AI_DATA_DIR']}))\n"
            " return 0\n")
        result = self.run_entry()
        self.assertEqual(0, result.returncode, result.stderr)
        self.assertEqual(1, len(result.stdout.splitlines()))
        record = json.loads(result.stdout)
        self.assertEqual('configured-backend', record['backend'])
        self.assertEqual(['probe', '--input', '/样本/片段.mkv'], record['args'])
        self.assertEqual(str(ROOT / 'outputs/apex-ai/model-data'), record['data_dir'])
        self.assertIn('backend import log', result.stderr)

    def test_explicit_missing_backend_does_not_fall_back(self):
        result = self.run_entry(self.folder / 'missing')
        self.assertNotEqual(0, result.returncode)
        self.assertEqual('backend_unavailable', json.loads(result.stdout)['code'])
        self.assertNotIn('Traceback', result.stderr)

    def test_missing_backend_dependency_is_structured(self):
        self.write_backend('import definitely_missing_filmcraft_test_dependency\n')
        result = self.run_entry()
        self.assertNotEqual(0, result.returncode)
        self.assertEqual('backend_import_failed', json.loads(result.stdout)['code'])
        self.assertNotIn('Traceback', result.stderr)

    def test_backend_failure_is_structured(self):
        self.write_backend("def main(argv, *, env):\n raise RuntimeError('controlled failure')\n")
        result = self.run_entry()
        self.assertNotEqual(0, result.returncode)
        self.assertEqual('backend_failed', json.loads(result.stdout)['code'])
        self.assertIn('controlled failure', json.loads(result.stdout)['message'])


if __name__ == '__main__':
    unittest.main()
