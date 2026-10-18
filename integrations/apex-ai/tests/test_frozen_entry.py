"""Exercise the public NDJSON entry with a relocated frozen-service fixture.

The launcher emulates PyInstaller's sys attributes; no real model or freeze is
used here. Packaging acceptance separately exercises the real executable.
"""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

ENTRY = Path(__file__).resolve().parents[1] / 'bridge.py'
FROZEN_ENTRY = ENTRY.with_name('frozen_entry.py')


class FrozenEntryTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name).resolve() / '搬迁 应用' / 'ai-runtime'
        self.service = self.root / 'service/filmcraft-ai'
        self.backend = self.root / 'service/_internal/backend/integrations/filmcraft/bridge.py'
        self.backend.parent.mkdir(parents=True)
        self.data = Path(self.temp.name).resolve() / '外接 数据'
        self.data.mkdir()
        self.manifest = {
            'schema_version': 1, 'platform': 'macos-arm64',
            'service': 'service/filmcraft-ai', 'model_workspace': 'model',
            'checkpoint': 'model/checkpoints/BEATs_strong_1.pt',
            'ffmpeg': 'service/_internal/bin/ffmpeg',
            'ffprobe': 'service/_internal/bin/ffprobe',
        }
        for relative in ('model/scripts/infer_apex_audio.py',
                         'model/checkpoints/BEATs_strong_1.pt',
                         'service/_internal/bin/ffmpeg', 'service/_internal/bin/ffprobe'):
            path = self.root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text('fixture')
            if path.name in ('ffmpeg', 'ffprobe'):
                path.chmod(0o755)
        (self.root / 'model/PretrainedSED').mkdir()
        self.save_manifest()
        self.service.write_text(
            '#!' + sys.executable + '\nimport runpy, sys\n'
            'sys.frozen = True\nsys.executable = __file__\n'
            'import os, plistlib, subprocess\n'
            'original_output = subprocess.check_output\n'
            'def disk_output(command, **kwargs):\n'
            ' if command[0] == "/sbin/mount": return ' + repr('/dev/disk99 on ' + str(Path(self.temp.name).resolve()) + ' (apfs, local, journaled)\n') + '\n'
            ' if command[0] == "/usr/bin/hdiutil": return plistlib.dumps({"images":[]})\n'
            ' if command[0] == "/usr/sbin/diskutil": return plistlib.dumps({"Internal":bool(os.environ.get("FILMCRAFT_TEST_INTERNAL_VOLUME"))})\n'
            ' return original_output(command, **kwargs)\n'
            'subprocess.check_output = disk_output\n'
            'sys.path.insert(0, ' + repr(str(ENTRY.parent)) + ')\n'
            'runpy.run_path(' + repr(str(ENTRY)) + ', run_name="__main__")\n')
        self.service.chmod(0o755)
        self.backend.write_text('''import json

def main(argv, *, env, service_factory):
 print(json.dumps({'type':'media', 'data':env['APEX_AI_DATA_DIR'],
  'model':env['APEX_MODEL_WORKSPACE'], 'ffmpeg':env['APEX_FFMPEG'],
  'cache':env['TORCH_HOME'], 'temporary':env['TMPDIR']}))
 return 0
class ModelService: pass
''')

    def tearDown(self):
        self.temp.cleanup()

    def save_manifest(self):
        (self.root / 'manifest.json').write_text(json.dumps(self.manifest))

    def run_service(self, command='probe', *, data=True, extra_env=None):
        env = dict(os.environ)
        for key in ('APEX_AI_DATA_DIR', 'FILMCRAFT_AI_DATA_DIR'):
            env.pop(key, None)
        if data:
            env['FILMCRAFT_AI_DATA_DIR'] = str(self.data)
        env['APEX_SERVICE_ROOT'] = '/nonexistent/developer/backend'
        env.update(extra_env or {})
        arguments = [command] if command.startswith('--') else [command, '--input', '/source.mkv']
        proc = subprocess.run([str(self.service), *arguments],
                              env=env, cwd=self.temp.name, capture_output=True,
                              text=True, timeout=10)
        return proc, [json.loads(line) for line in proc.stdout.splitlines()]

    def test_relocated_probe_uses_bundled_resources_and_explicit_writable_data(self):
        proc, events = self.run_service()
        self.assertEqual(0, proc.returncode, proc.stdout + proc.stderr)
        self.assertEqual([{'type': 'media', 'data': str(self.data),
                           'model': str(self.root / 'model'),
                           'ffmpeg': str(self.root / 'service/_internal/bin/ffmpeg'),
                           'cache': str(self.data / 'cache/torch'),
                           'temporary': str(self.data / 'tmp')}], events)

    def test_missing_declared_model_loader_fails_without_development_fallback(self):
        (self.root / 'model/scripts/infer_apex_audio.py').unlink()
        proc, events = self.run_service()
        self.assertNotEqual(0, proc.returncode)
        self.assertEqual('runtime_unavailable', events[-1]['code'])
        self.assertFalse(any(e['type'] == 'media' for e in events))

    def test_cache_symlink_cannot_redirect_writes_outside_explicit_data_directory(self):
        outside = Path(self.temp.name) / 'unexpected-local-cache'
        outside.mkdir()
        (self.data / 'cache').symlink_to(outside, target_is_directory=True)
        proc, events = self.run_service()
        self.assertNotEqual(0, proc.returncode)
        self.assertEqual('runtime_unavailable', events[-1]['code'])
        self.assertEqual([], list(outside.iterdir()))

    def test_internal_data_volume_is_rejected_before_cache_creation(self):
        proc, events = self.run_service(extra_env={'FILMCRAFT_TEST_INTERNAL_VOLUME': '1'})
        self.assertNotEqual(0, proc.returncode)
        self.assertEqual('runtime_unavailable', events[-1]['code'])
        self.assertEqual([], list(self.data.iterdir()))

    def test_packaged_data_cannot_write_anywhere_inside_the_signed_app(self):
        app = self.root.parent / 'FilmCraft AI.app'
        packaged_root = app / 'Contents/Resources/ai-runtime'
        packaged_root.parent.mkdir(parents=True)
        shutil.move(str(self.root), packaged_root)
        self.root = packaged_root
        self.service = self.root / 'service/filmcraft-ai'
        sibling = app / 'Contents/Resources/native-data'
        sibling.mkdir()
        before = sorted(str(path.relative_to(app)) for path in app.rglob('*'))
        for data in (app, app / 'Contents', packaged_root.parent, sibling, self.root):
            for command in ('--check-runtime', 'probe', 'analyze', 'validate'):
                with self.subTest(data=data.relative_to(app), command=command):
                    self.data = data
                    proc, events = self.run_service(command)
                    self.assertNotEqual(0, proc.returncode, proc.stdout + proc.stderr)
                    self.assertEqual('runtime_unavailable', events[-1]['code'])
                    self.assertEqual(before, sorted(str(path.relative_to(app)) for path in app.rglob('*')))

    def test_frozen_worker_reentry_preserves_the_worker_protocol(self):
        self.service.write_text(self.service.read_text().replace(str(ENTRY), str(FROZEN_ENTRY)))
        # The freeze includes this module. This controlled worker boundary proves
        # dispatch; real torch/model inference belongs to package acceptance.
        (self.root / 'service/_internal/model_inference.py').write_text(
            'import json\n'
            'def main(): print(json.dumps({"type":"worker-ready"}))\n')
        self.service.write_text(self.service.read_text().replace(
            'import runpy, sys',
            'import runpy, sys\nsys.path.insert(0, __file__.rsplit("/", 1)[0] + "/_internal")'))
        proc, events = self.run_service('--inference-worker')
        self.assertEqual(0, proc.returncode, proc.stdout + proc.stderr)
        self.assertEqual([{'type':'worker-ready'}], events)

    def test_startup_check_validates_runtime_without_importing_the_backend(self):
        self.backend.write_text('raise AssertionError("startup check imported model backend")\n')
        proc, events = self.run_service('--check-runtime')
        self.assertEqual(0, proc.returncode, proc.stdout + proc.stderr)
        self.assertEqual([{'type':'runtime', 'status':'ready', 'data_dir':str(self.data)}], events)

    def test_missing_extra_declared_resource_prevents_public_probe(self):
        self.manifest['required_files'] = ['model/PretrainedSED/config.py']
        self.save_manifest()
        proc, events = self.run_service()
        self.assertNotEqual(0, proc.returncode)
        self.assertEqual('runtime_unavailable', events[-1]['code'])

    def test_missing_explicit_data_directory_fails_for_every_public_request(self):
        for command in ('probe', 'analyze', 'validate'):
            with self.subTest(command=command):
                proc, events = self.run_service(command, data=False)
                self.assertNotEqual(0, proc.returncode)
                self.assertEqual('runtime_unavailable', events[-1]['code'])
                self.assertIn('APEX_AI_DATA_DIR', events[-1]['message'])
                self.assertFalse(any(e['type'] in ('media', 'result', 'validation') for e in events))

    def test_manifest_cannot_reference_absolute_or_escaping_checkpoint_paths(self):
        outside = Path(self.temp.name) / 'BEATs_strong_1.pt'
        outside.write_bytes(b'outside')
        for value in (str(outside), '../../BEATs_strong_1.pt'):
            with self.subTest(value=value):
                self.manifest['checkpoint'] = value
                self.save_manifest()
                proc, events = self.run_service()
                self.assertNotEqual(0, proc.returncode)
                self.assertEqual('runtime_unavailable', events[-1]['code'])

    def test_guardian_reentry_forwards_its_parent_and_command(self):
        self.service.write_text(self.service.read_text().replace(str(ENTRY), str(FROZEN_ENTRY)))
        (self.root / 'service/_internal/model_runtime.py').write_text(
            'import json\n'
            'def guardian_main(argv): print(json.dumps({"type":"guardian", "argv":argv})); return 0\n')
        self.service.write_text(self.service.read_text().replace(
            'import runpy, sys',
            'import runpy, sys\nsys.path.insert(0, __file__.rsplit("/", 1)[0] + "/_internal")'))
        env = dict(os.environ)
        proc = subprocess.run([str(self.service), '--process-guardian', '123', '--', 'worker'],
                              env=env, capture_output=True, text=True, timeout=10)
        self.assertEqual(0, proc.returncode, proc.stdout + proc.stderr)
        self.assertEqual({'type':'guardian', 'argv':['123', '--', 'worker']}, json.loads(proc.stdout))

    def test_analyze_reenters_frozen_worker_and_preserves_failure_progress_without_result(self):
        self.service.write_text(self.service.read_text().replace(str(ENTRY), str(FROZEN_ENTRY)))
        self.service.write_text(self.service.read_text().replace(
            'import runpy, sys',
            'import runpy, sys\nsys.path.insert(0, __file__.rsplit("/", 1)[0] + "/_internal")'))
        (self.root / 'service/_internal/model_inference.py').write_text(
            'import json\n'
            'def main():\n'
            ' print(json.dumps({"type":"progress", "stage":"model_loading"}))\n'
            ' print(json.dumps({"type":"error", "code":"model_load_failed", "message":"controlled worker failure"}))\n')
        self.backend.write_text('''import subprocess
class ModelService:
 def __init__(self, *, model_command):
  self.worker = subprocess.run(model_command, capture_output=True, text=True, timeout=10)
def main(argv, *, env, output, service_factory):
 service = service_factory()
 output.write(service.worker.stdout)
 return 1
''')
        proc, events = self.run_service('analyze')
        self.assertNotEqual(0, proc.returncode)
        self.assertEqual([{'type':'progress', 'stage':'model_loading'},
                          {'type':'error', 'code':'model_load_failed',
                           'message':'controlled worker failure'}], events)

    def test_loading_bundled_backend_does_not_write_bytecode_into_signed_resources(self):
        self.service.write_text(self.service.read_text().replace('sys.frozen = True', 'sys.frozen = True; sys.pycache_prefix = None'))
        helper = self.root / 'service/_internal/backend/backend_helper.py'
        helper.write_text('NAME = "bundled helper"\n')
        self.backend.write_text('import backend_helper\n' + self.backend.read_text())
        proc, events = self.run_service()
        self.assertEqual(0, proc.returncode, proc.stdout + proc.stderr)
        self.assertEqual([], list(self.root.rglob('*.pyc')))


if __name__ == '__main__':
    unittest.main()
