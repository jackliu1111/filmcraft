#!/usr/bin/env python3
"""Read-only preflight and inside-out native-code inventory for AI app packaging."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess

MAGICS = {bytes.fromhex(value) for value in ('cffaedfe', 'feedfacf', 'cefaedfe', 'feedface',
                                            'cafebabe', 'bebafeca', 'cafebabf', 'bfbafeca')}


def native(path):
    if path.is_symlink() or not path.is_file():
        return False
    with path.open('rb') as stream:
        return stream.read(4) in MAGICS


def resources(root):
    if not root.is_absolute() or not root.is_dir():
        raise ValueError('AI runtime must be an existing absolute directory')
    root = root.resolve()
    manifest = json.loads((root / 'manifest.json').read_text(encoding='utf-8'))
    if manifest.get('schema_version') != 1 or manifest.get('platform') != 'macos-arm64':
        raise ValueError('AI runtime requires schema_version 1 and platform macos-arm64')
    expected = {'service': 'service/filmcraft-ai', 'model_workspace': 'model',
                'checkpoint': 'model/checkpoints/BEATs_strong_1.pt',
                'ffmpeg': 'service/_internal/bin/ffmpeg', 'ffprobe': 'service/_internal/bin/ffprobe'}
    for key, value in expected.items():
        if manifest.get(key) != value:
            raise ValueError('AI runtime contract mismatch: ' + key)
    required = manifest.get('required_files')
    if not isinstance(required, list) or not required:
        raise ValueError('AI runtime must declare required_files')
    mandatory = ['model/scripts/infer_apex_audio.py', 'model/PretrainedSED/config.py',
                 'model/PretrainedSED/LICENSE', 'service/_internal/backend/integrations/filmcraft/bridge.py',
                 'service/_internal/model_inference.py']
    for value in list(expected.values()) + mandatory + required:
        if not isinstance(value, str) or not value or Path(value).is_absolute():
            raise ValueError('AI runtime resources must use relative paths')
        path = root / value
        if not path.resolve().is_relative_to(root) or not path.exists():
            raise ValueError('AI runtime resource missing or escapes root: ' + value)
        if value != expected['model_workspace'] and not path.is_file():
            raise ValueError('AI runtime requires a regular file: ' + value)
    if not (root / expected['model_workspace']).is_dir():
        raise ValueError('AI runtime model workspace must be a directory')
    for path in root.rglob('*'):
        if path.is_symlink() and (not path.exists() or not path.resolve().is_relative_to(root)):
            raise ValueError('AI runtime symlink missing or escapes root: ' + str(path))
    for key in ('service', 'ffmpeg', 'ffprobe'):
        path = root / expected[key]
        if not os.access(path, os.X_OK) or not native(path):
            raise ValueError('AI runtime program must be executable native code: ' + key)
    for path in root.rglob('*'):
        if native(path):
            arches = subprocess.check_output(['/usr/bin/lipo', '-archs', str(path)], text=True).split()
            if 'arm64' not in arches:
                raise ValueError('AI runtime native code lacks arm64: ' + str(path))
    return root


def preflight(args):
    # Reject unsupported architectures before invoking build, lipo, or removing old output.
    if args.arch != 'aarch64':
        raise ValueError('AI runtime supports only --arch aarch64; x86_64/universal are unsupported')
    root = resources(args.runtime)
    source = Path(__file__).resolve().parents[1] / 'ai/build_runtime.py'
    spec = importlib.util.spec_from_file_location('filmcraft_runtime_build', source)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    size = sum(path.stat().st_size for path in root.rglob('*') if path.is_file() and not path.is_symlink())
    # Work app, DMG staging, image conversion and final archive can coexist.
    native_bytes = sum(path.stat().st_size for name in ('filmcraft', 'filmcraft-cli')
                       for path in (args.target_dir / 'aarch64-apple-darwin/release' / name,) if path.is_file())
    minimum = 1024**3 + (size + native_bytes) * 5
    for path in (args.target_dir, args.dist):
        if not path.is_absolute():
            raise ValueError('AI runtime package output directories must be absolute')
        path = path.resolve()
        module.external_storage(path)
        if path.is_relative_to(root) or root.is_relative_to(path / 'macos-package'):
            raise ValueError('AI runtime input overlaps destructive package work directory')
        parent = path
        while not parent.exists():
            parent = parent.parent
        if not os.access(parent, os.W_OK) or shutil.disk_usage(parent).free < minimum:
            raise ValueError(f'AI runtime packaging needs writable external storage and {minimum} free bytes: {parent}')
    print(f'==> AI runtime preflight passed ({size} bytes; at least {minimum} free bytes required)')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='command', required=True)
    check = sub.add_parser('check')
    check.add_argument('--runtime', type=Path, required=True)
    check.add_argument('--arch', required=True)
    check.add_argument('--target-dir', type=Path, required=True)
    check.add_argument('--dist', type=Path, required=True)
    listing = sub.add_parser('signing-paths')
    listing.add_argument('runtime', type=Path)
    args = parser.parse_args()
    try:
        if args.command == 'check':
            preflight(args)
        else:
            # First sign each real Mach-O, then deepest framework/app bundles, then their parents.
            files, bundles = [], []
            for path in args.runtime.rglob('*'):
                if native(path):
                    files.append(path)
                elif not path.is_symlink() and path.is_dir() and path.suffix in ('.framework', '.app', '.xpc', '.bundle'):
                    bundles.append(path)
            for path in sorted(files, key=lambda p: len(p.parts), reverse=True) + sorted(bundles, key=lambda p: len(p.parts), reverse=True):
                os.write(1, os.fsencode(path) + b'\0')
    except (OSError, ValueError, TypeError, AttributeError, subprocess.SubprocessError) as exc:
        parser.error('AI runtime: ' + str(exc))


if __name__ == '__main__':
    main()
