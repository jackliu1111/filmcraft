#!/usr/bin/env python3
"""Read-only preflight and inside-out native-code inventory for AI app packaging."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import plistlib
import re
import shutil
import subprocess
import sys

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
    return root


def inspect_runtime(root):
    """Read arm64 load commands rather than trusting a build-host or manifest version."""
    if not root.is_dir():
        raise ValueError('AI runtime must be an existing directory')
    versions = [(11, 0, 0)]
    count = 0
    for path in root.rglob('*'):
        if not native(path):
            continue
        output = subprocess.check_output(['/usr/bin/vtool', '-arch', 'arm64', '-show-build', str(path)], text=True)
        found = []
        for command in re.split(r'(?m)^Load command \d+\n', output)[1:]:
            if 'cmd LC_BUILD_VERSION' in command and re.search(r'\bplatform MACOS\b', command):
                found += re.findall(r'(?m)^\s*minos ([0-9]+(?:\.[0-9]+){1,2})\s*$', command)
            elif 'cmd LC_VERSION_MIN_MACOSX' in command:
                found += re.findall(r'(?m)^\s*version ([0-9]+(?:\.[0-9]+){1,2})\s*$', command)
        if not found:
            raise ValueError('AI runtime native code lacks a macOS arm64 deployment version: ' + str(path))
        for version in found:
            components = tuple(int(part) for part in version.split('.'))
            versions.append(components + (0,) * (3 - len(components)))
        count += 1
    if not count:
        raise ValueError('AI runtime contains no native code to inspect')
    major, minor, patch = max(versions)
    minimum = f'{major}.{minor}' + (f'.{patch}' if patch else '')
    return {'minimum_system_version': minimum, 'native_file_count': count}


def preflight(args):
    # Reject unsupported architectures before invoking build, lipo, or removing old output.
    if args.arch != 'aarch64':
        raise ValueError('AI runtime supports only --arch aarch64; x86_64/universal are unsupported')
    root = resources(args.runtime)
    deployment = inspect_runtime(root)
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
    print(f'==> AI runtime preflight passed ({size} bytes; at least {minimum} free bytes required; '
          f'macOS {deployment["minimum_system_version"]} or later)', file=sys.stderr)
    print(deployment['minimum_system_version'])


def create_image(stage, output):
    """Preserve signed resources, then verify the app from the read-only image."""
    if not stage.is_dir() or not (stage / 'FilmCraft.app').is_dir():
        raise ValueError('DMG stage must contain FilmCraft.app')
    if not output.is_absolute() or output.suffix != '.dmg' or output.exists():
        raise ValueError('DMG output must be a new absolute .dmg path')
    # package preflight has verified external output storage. Keep hdiutil's
    # temporary copies beside that output, including its potentially large image.
    environment = os.environ | {'TMPDIR': str(output.parent)}
    subprocess.run(['/usr/bin/hdiutil', 'create', '-srcfolder', str(stage),
                    '-fs', 'APFS', '-volname', 'FilmCraft', '-format', 'UDZO',
                    '-imagekey', 'zlib-level=9', str(output)],
                   env=environment, check=True, timeout=1800)
    attached = plistlib.loads(subprocess.check_output([
        '/usr/bin/hdiutil', 'attach', '-readonly', '-nobrowse', '-plist', str(output)],
        env=environment, timeout=120))
    entities = attached.get('system-entities', [])
    device = next((entity['dev-entry'] for entity in entities if 'dev-entry' in entity), None)
    try:
        apps = [Path(entity['mount-point']) / 'FilmCraft.app'
                for entity in entities if 'mount-point' in entity]
        apps = [app for app in apps if app.is_dir()]
        if len(apps) != 1 or device is None:
            raise ValueError('Final read-only DMG must contain exactly one mounted FilmCraft.app')
        subprocess.run(['/usr/bin/codesign', '--verify', '--strict', '--deep',
                        '--verbose=2', str(apps[0])], check=True, timeout=120)
    finally:
        if device:
            subprocess.run(['/usr/bin/hdiutil', 'detach', device], check=True, timeout=120)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='command', required=True)
    check = sub.add_parser('check')
    check.add_argument('--runtime', type=Path, required=True)
    check.add_argument('--arch', required=True)
    check.add_argument('--target-dir', type=Path, required=True)
    check.add_argument('--dist', type=Path, required=True)
    inspection = sub.add_parser('inspect', help='Read-only native deployment-version inventory')
    inspection.add_argument('runtime', type=Path)
    listing = sub.add_parser('signing-paths')
    listing.add_argument('runtime', type=Path)
    smoke = sub.add_parser('smoke', help='Start the signed AI service and media tools without importing the model')
    smoke.add_argument('runtime', type=Path)
    smoke.add_argument('--data', type=Path, required=True,
                       help='Existing external data directory for the startup check')
    image = sub.add_parser('image', help='Create an APFS DMG and verify its read-only mounted app')
    image.add_argument('--stage', type=Path, required=True)
    image.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    try:
        if args.command == 'check':
            preflight(args)
        elif args.command == 'inspect':
            print(json.dumps(inspect_runtime(args.runtime)))
        elif args.command == 'image':
            create_image(args.stage, args.output)
        elif args.command == 'smoke':
            for relative, argument in (('service/filmcraft-ai', '--check-runtime'),
                                       ('service/_internal/bin/ffmpeg', '-version'),
                                       ('service/_internal/bin/ffprobe', '-version')):
                subprocess.run([str(args.runtime / relative), argument],
                               env=os.environ | {'APEX_AI_DATA_DIR': str(args.data),
                                                 'FILMCRAFT_AI_DATA_DIR': str(args.data)},
                               check=True, timeout=60)
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
