"""Resolve the read-only frozen runtime and its explicitly configured data root."""
import json
import os
import plistlib
import re
import subprocess
from pathlib import Path
import sys
import tempfile


class RuntimeUnavailable(Exception):
    code = 'runtime_unavailable'


def external_data_volume(path):
    """Verify physical storage, including disk images backed by external disks."""
    mounts = []
    for line in subprocess.check_output(['/sbin/mount'], text=True, timeout=10).splitlines():
        match = re.match(r'(/dev/\S+) on (.+) \(', line)
        if match:
            mounts.append(Path(match[2]).resolve())
    images = plistlib.loads(subprocess.check_output(['/usr/bin/hdiutil', 'info', '-plist'], timeout=10))

    def verify(candidate, seen):
        roots = [root for root in mounts if candidate.is_relative_to(root)]
        if not roots:
            raise ValueError('数据目录不在已挂载的外接卷上')
        root = max(roots, key=lambda value: len(value.parts))
        if root in seen:
            raise ValueError('外接数据卷的磁盘映像来源无效')
        for image in images.get('images', []):
            if any(entity.get('mount-point') == str(root) for entity in image.get('system-entities', [])):
                return verify(Path(image['image-path']).resolve(), seen | {root})
        info = plistlib.loads(subprocess.check_output(['/usr/sbin/diskutil', 'info', '-plist', str(root)], timeout=10))
        if info.get('Internal') is not False:
            raise ValueError('数据目录必须位于可验证的外接卷上')
        return root

    return verify(path, set())


def runtime_environment(env):
    root = Path(sys.executable).resolve().parent.parent
    try:
        manifest = json.loads((root / 'manifest.json').read_text(encoding='utf-8'))
        if manifest.get('schema_version') != 1 or manifest.get('platform') != 'macos-arm64':
            raise ValueError('不支持的 AI 运行时版本或平台')

        def resource(key, *, directory=False):
            value = manifest.get(key)
            if not isinstance(value, str) or not value or Path(value).is_absolute():
                raise ValueError('AI 运行时路径必须相对包目录：' + key)
            path = (root / value).resolve()
            if not path.is_relative_to(root) or not (path.is_dir() if directory else path.is_file()):
                raise ValueError('AI 运行时资源缺失或越出包目录：' + key)
            return path

        required_files = manifest.get('required_files', [])
        if not isinstance(required_files, list):
            raise ValueError('AI 运行时 required_files 必须是文件路径列表')
        for value in required_files:
            if not isinstance(value, str) or not value or Path(value).is_absolute():
                raise ValueError('AI 运行时文件须相对包目录')
            path = (root / value).resolve()
            if not path.is_relative_to(root) or not path.is_file():
                raise ValueError('AI 运行时声明的文件缺失或越出包目录：' + value)
        service = resource('service')
        if service != Path(sys.executable).resolve():
            raise ValueError('AI 运行时服务与清单不一致')
        model = resource('model_workspace', directory=True)
        checkpoint = resource('checkpoint')
        ffmpeg, ffprobe = resource('ffmpeg'), resource('ffprobe')
        if not all(os.access(path, os.X_OK) for path in (service, ffmpeg, ffprobe)):
            raise ValueError('AI 运行时程序不可执行')
        backend = root / 'service/_internal/backend'
        for path, directory in ((backend / 'integrations/filmcraft/bridge.py', False),
                                (model / 'scripts/infer_apex_audio.py', False),
                                (model / 'PretrainedSED', True)):
            resolved = path.resolve()
            if not resolved.is_relative_to(root) or not (resolved.is_dir() if directory else resolved.is_file()):
                raise ValueError('AI 运行时资源缺失或越出包目录：' + str(path))
        if checkpoint.name != 'BEATs_strong_1.pt' or not checkpoint.is_relative_to(model):
            raise ValueError('AI 运行时权重须位于模型目录并名为 BEATs_strong_1.pt')
        supplied = env.get('APEX_AI_DATA_DIR') or env.get('FILMCRAFT_AI_DATA_DIR')
        if not supplied:
            raise ValueError('请显式设置 APEX_AI_DATA_DIR 或 FILMCRAFT_AI_DATA_DIR 为可写的外接数据目录')
        data = Path(supplied).expanduser()
        # A packaged runtime cannot write anywhere in its enclosing signed app,
        # including ancestors/siblings of ai-runtime. Staged runtimes protect root.
        readonly_root = next((parent for parent in reversed(root.parents)
                              if parent.suffix.lower() == '.app'), root)
        if not data.is_absolute() or not data.is_dir() or data.resolve().is_relative_to(readonly_root):
            raise ValueError('AI 数据目录须为包目录之外已存在的绝对目录')
        data = data.resolve()
        external_data_volume(data)
        env = dict(env, APEX_AI_DATA_DIR=str(data), APEX_SERVICE_ROOT=str(backend),
                   APEX_MODEL_WORKSPACE=str(model), APEX_CHECKPOINT=str(checkpoint),
                   APEX_FFMPEG=str(ffmpeg), APEX_FFPROBE=str(ffprobe),
                   APEX_ENGINE_PYTHON=str(service))
        for key, relative in (('TORCH_HOME', 'cache/torch'), ('HF_HOME', 'cache/huggingface'),
                              ('XDG_CACHE_HOME', 'cache'), ('NUMBA_CACHE_DIR', 'cache/numba'),
                              ('MPLCONFIGDIR', 'cache/matplotlib'), ('TMPDIR', 'tmp'),
                              ('TMP', 'tmp'), ('TEMP', 'tmp')):
            path = data / relative
            if not path.resolve().is_relative_to(data):
                raise ValueError('AI 缓存或临时目录不能越出数据目录：' + str(path))
            path.mkdir(parents=True, exist_ok=True)
            env[key] = str(path)
        with tempfile.TemporaryFile(dir=data / 'tmp'):
            pass
        # Dynamic model/backend source imports must never mutate signed resources.
        env['PYTHONDONTWRITEBYTECODE'] = '1'
        sys.dont_write_bytecode = True
        os.environ.update(env)
        tempfile.tempdir = env['TMPDIR']
        return env
    except (OSError, ValueError, TypeError, AttributeError, KeyError, subprocess.SubprocessError) as exc:
        raise RuntimeUnavailable('包内 AI 运行时不可用：' + str(exc)) from exc


class FrozenBackend:
    """Use the core's public factory seam for the frozen inference command."""
    def __init__(self, backend):
        self.backend = backend

    def __getattr__(self, name):
        return getattr(self.backend, name)

    def ModelService(self, **arguments):
        return self.backend.ModelService(**arguments, model_command=[sys.executable, '--inference-worker'])

    def main(self, argv, **arguments):
        return self.backend.main(argv, **arguments, service_factory=self.ModelService)
