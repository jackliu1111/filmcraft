"""Public packaging/launch contracts, using tiny OS-process boundary fixtures."""
import json
import os
import plistlib
import shutil
import sys
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[3]
PACKAGE = ROOT / 'packaging/macos/package.sh'
LAUNCHER = ROOT / 'packaging/macos/ai-launcher.sh'


class PackageTests(unittest.TestCase):
    def test_signed_runtime_smoke_checks_bundled_media_loaders_before_distribution(self):
        for broken in ('ffmpeg', 'ffprobe', None):
            with self.subTest(broken=broken), tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                runtime = root / 'FilmCraft AI.app/Contents/Resources/ai-runtime'
                programs = (('service/filmcraft-ai', '--check-runtime'),
                            ('service/_internal/bin/ffmpeg', '-version'),
                            ('service/_internal/bin/ffprobe', '-version'))
                for relative, argument in programs:
                    program = runtime / relative
                    program.parent.mkdir(parents=True, exist_ok=True)
                    program.write_text('#!/bin/bash\n[ "$1" = ' + argument + ' ] || exit 91\n' +
                                       ('echo "media loader rejected: different Team IDs" >&2\nexit 255\n'
                                        if program.name == broken else 'echo "' + program.name + ':' + argument + '"\n'))
                    program.chmod(0o755)
                result = subprocess.run([sys.executable, str(ROOT / 'packaging/macos/ai-package.py'),
                                         'smoke', str(runtime), '--data', str(root / 'ai-signing-smoke')],
                                        capture_output=True, text=True, timeout=10)
                if broken:
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn('different Team IDs', result.stderr)
                else:
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(result.stdout.splitlines(), [
                        'filmcraft-ai:--check-runtime', 'ffmpeg:-version', 'ffprobe:-version'])

    def test_signed_runtime_smoke_surfaces_loader_failure_before_distribution(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            runtime = root / 'FilmCraft AI.app/Contents/Resources/ai-runtime'
            service = runtime / 'service/filmcraft-ai'
            service.parent.mkdir(parents=True)
            service.write_text('#!/bin/bash\necho "signed Python loader rejected: different Team IDs" >&2\nexit 255\n')
            service.chmod(0o755)
            result = subprocess.run([sys.executable, str(ROOT / 'packaging/macos/ai-package.py'),
                                     'smoke', str(runtime), '--data', str(root / 'ai-signing-smoke')],
                                    capture_output=True, text=True, timeout=10)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('different Team IDs', result.stderr)

    def test_ai_architecture_mismatch_stops_before_resource_or_build_work(self):
        for arch in ('universal', 'x86_64'):
            with self.subTest(arch=arch), tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                result = subprocess.run(['bash', str(PACKAGE), '--skip-build', '--arch', arch,
                                         '--ai-runtime', str(root / 'missing-runtime')],
                                        env=os.environ | {'DIST': str(root / 'dist'),
                                                          'CARGO_TARGET_DIR': str(root / 'target')},
                                        capture_output=True, text=True)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn('only --arch aarch64', result.stderr)
                self.assertFalse((root / 'dist').exists())
                self.assertFalse((root / 'target').exists())

    def test_invalid_runtime_leaves_existing_package_and_dist_untouched(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            target = root / 'target'
            old = target / 'macos-package/FilmCraft.app/keep-me'
            old.parent.mkdir(parents=True)
            old.write_text('previous package')
            dist = root / 'new-dist'
            result = subprocess.run(['bash', str(PACKAGE), '--skip-build', '--arch', 'aarch64',
                                     '--ai-runtime', str(root / 'missing-runtime')],
                                    env=os.environ | {'DIST': str(dist), 'CARGO_TARGET_DIR': str(target)},
                                    capture_output=True, text=True)
            self.assertIn('AI runtime', result.stderr)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(old.read_text(), 'previous package')
            self.assertFalse(dist.exists())


@unittest.skipUnless(sys.platform == 'darwin' and shutil.which('clang'), 'macOS native tools required')
class RuntimeInspectionTests(unittest.TestCase):
    def test_inspection_reports_highest_embedded_arm64_deployment_version(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            source = root / 'main.c'
            source.write_text('int main(void) { return 0; }\n')
            runtime = root / 'ai-runtime'
            runtime.mkdir()
            low = runtime / 'service'
            high = runtime / 'ffmpeg'
            subprocess.run(['/usr/bin/clang', '-arch', 'arm64', '-mmacosx-version-min=11.0',
                            str(source), '-o', str(low)], check=True, capture_output=True)
            subprocess.run(['/usr/bin/vtool', '-set-build-version', 'macos', '27.0', '27.0',
                            '-replace', '-output', str(high), str(low)], check=True, capture_output=True)
            result = subprocess.run([sys.executable, str(ROOT / 'packaging/macos/ai-package.py'),
                                     'inspect', str(runtime)], capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(json.loads(result.stdout)['minimum_system_version'], '27.0')


@unittest.skipUnless(sys.platform == 'darwin' and os.environ.get('FILMCRAFT_AI_TEST_VOLUME'),
                     'requires an explicitly configured external APFS test volume')
class DmgImageTests(unittest.TestCase):
    def test_image_preserves_signed_app_and_rejects_invalid_final_resources(self):
        with tempfile.TemporaryDirectory(dir=os.environ['FILMCRAFT_AI_TEST_VOLUME']) as temp:
            root = Path(temp)
            stage = root / 'stage'
            app = stage / 'FilmCraft.app'
            binary = app / 'Contents/MacOS/FilmCraft'
            binary.parent.mkdir(parents=True)
            (app / 'Contents/Info.plist').write_bytes(plistlib.dumps({
                'CFBundleExecutable': 'FilmCraft', 'CFBundleIdentifier': 'test.filmcraft.dmg',
                'CFBundleName': 'FilmCraft', 'CFBundlePackageType': 'APPL', 'CFBundleVersion': '1'}))
            source = root / 'main.c'
            source.write_text('int main(void) { return 0; }\n')
            subprocess.run(['/usr/bin/clang', '-arch', 'arm64', str(source), '-o', str(binary)],
                           check=True, capture_output=True)
            subprocess.run(['/usr/bin/codesign', '--force', '--sign', '-', str(app)],
                           check=True, capture_output=True)
            subprocess.run(['/usr/bin/codesign', '--verify', '--strict', '--deep', str(app)],
                           check=True, capture_output=True)
            command = [sys.executable, str(ROOT / 'packaging/macos/ai-package.py'),
                       'image', '--stage', str(stage), '--output']
            result = subprocess.run(command + [str(root / 'valid.dmg')], capture_output=True, text=True, timeout=120)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertTrue((root / 'valid.dmg').is_file())
            resources = app / 'Contents/Resources'
            resources.mkdir()
            (resources / 'unsigned.txt').write_text('changed after signing')
            result = subprocess.run(command + [str(root / 'invalid.dmg')], capture_output=True, text=True, timeout=120)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('sealed resource', result.stderr)


class LauncherTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='FilmCraft 空格 ')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.app = self.root / '搬迁后/FilmCraft AI.app'
        self.macos = self.app / 'Contents/MacOS'
        self.macos.mkdir(parents=True)
        self.launcher = self.macos / 'FilmCraft'
        if LAUNCHER.exists():
            self.launcher.write_bytes(LAUNCHER.read_bytes())
        self.data = self.root / '外接数据目录'
        self.data.mkdir()
        self.service = self.app / 'Contents/Resources/ai-runtime/service/filmcraft-ai'
        self.service.parent.mkdir(parents=True)
        self.service.write_text('#!/bin/bash\n[ "$1" = --check-runtime ] || exit 91\n'
                                '[ -d "$APEX_AI_DATA_DIR" ] || { echo "invalid external data"; exit 3; }\n'
                                'mkdir -p "$APEX_AI_DATA_DIR/tmp"\n'
                                "echo '{\"type\":\"runtime\",\"status\":\"ready\"}'\n")
        self.service.chmod(0o755)
        core = self.macos / 'FilmCraft-core'
        core.write_text('#!/bin/bash\nprintf "%s\\n" "$APEX_AI_DATA_DIR" "$FILMCRAFT_DATA_DIR" "$TMPDIR" "$HF_HOME" "$@"\n')
        core.chmod(0o755)
        self.env = {key: value for key, value in os.environ.items()
                    if not key.startswith(('APEX_', 'FILMCRAFT_'))}
        self.env.update(HOME=str(self.root / 'home'), PATH='/usr/bin:/bin',
                        FILMCRAFT_AI_DATA_DIR=str(self.data))

    def run_launcher(self, *args, **env):
        return subprocess.run(['/bin/bash', str(self.launcher), *args],
                              env=self.env | env, capture_output=True, text=True, timeout=10)

    def test_relocated_app_launches_with_explicit_external_data_and_preserves_arguments(self):
        result = self.run_launcher('素材 path.mp4', '--example')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.splitlines(), [str(self.data), str(self.data / 'filmcraft'),
                         str(self.data / 'tmp'), str(self.data / 'cache/huggingface'),
                         '素材 path.mp4', '--example'])

    def test_explicit_invalid_data_stops_before_native_start_without_dialog(self):
        result = self.run_launcher(FILMCRAFT_AI_DATA_DIR=str(self.root / 'missing-data'))
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('invalid external data', result.stderr)
        self.assertEqual(result.stdout, '')

    def test_remembered_literal_path_launches_without_terminal_environment(self):
        data = self.root / "数据 $HOME ' literal"
        data.mkdir()
        preference = Path(self.env['HOME']) / 'Library/Application Support/FilmCraft/ai-data-directory'
        preference.parent.mkdir(parents=True)
        preference.write_bytes(os.fsencode(data) + b'\0')
        result = self.run_launcher(FILMCRAFT_AI_DATA_DIR='')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.splitlines()[0], str(data))
        self.assertEqual(preference.read_bytes(), os.fsencode(data) + b'\0')

    def test_version_works_without_data_or_runtime_and_never_prompts(self):
        self.service.unlink()
        result = self.run_launcher('--version', FILMCRAFT_AI_DATA_DIR='')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(result.stdout.endswith('--version\n'))

    def test_developer_overrides_launch_without_frozen_runtime_and_keep_explicit_options(self):
        self.service.unlink()
        core = self.macos / 'FilmCraft-core'
        core.write_text('#!' + sys.executable + '\nimport json, os, sys\n'
                        'print(json.dumps({"args":sys.argv[1:], "env":{key:os.environ.get(key) '
                        'for key in ("FILMCRAFT_APEX_PYTHON", "FILMCRAFT_APEX_BRIDGE", '
                        '"APEX_AI_DATA_DIR", "FILMCRAFT_DATA_DIR", "TMPDIR", "HF_HOME")}}))\n')
        options = {'APEX_AI_DATA_DIR': str(self.data),
                   'FILMCRAFT_DATA_DIR': str(self.data / 'native configured'),
                   'TMPDIR': str(self.data / 'temporary configured'),
                   'HF_HOME': str(self.data / 'cache configured')}
        for overrides in ({'FILMCRAFT_APEX_PYTHON': '/developer path/python'},
                          {'FILMCRAFT_APEX_BRIDGE': '/developer path/bridge.py'},
                          {'FILMCRAFT_APEX_PYTHON': '', 'FILMCRAFT_APEX_BRIDGE': ''}):
            with self.subTest(overrides=overrides):
                result = self.run_launcher('素材 path.mp4', **options, **overrides)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(json.loads(result.stdout), {
                    'args': ['素材 path.mp4'], 'env': options | {
                        'FILMCRAFT_APEX_PYTHON': overrides.get('FILMCRAFT_APEX_PYTHON'),
                        'FILMCRAFT_APEX_BRIDGE': overrides.get('FILMCRAFT_APEX_BRIDGE')}})
                self.assertEqual([], list(self.data.iterdir()))
        result = self.run_launcher('素材 path.mp4', **options)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('包内 AI 服务缺失或不可执行', result.stderr)
        self.assertEqual(result.stdout, '')

    def test_apex_data_override_has_priority_over_filmcraft_data(self):
        result = self.run_launcher(APEX_AI_DATA_DIR=str(self.data),
                                   FILMCRAFT_AI_DATA_DIR=str(self.root / 'missing-data'))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.splitlines()[0], str(self.data))


if __name__ == '__main__':
    unittest.main()
