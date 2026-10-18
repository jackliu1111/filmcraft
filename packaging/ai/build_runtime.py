#!/usr/bin/env python3
"""Freeze FilmCraft AI on macOS arm64, directly onto an external APFS volume."""
import argparse
import hashlib
from importlib import metadata, util
import json
import os
from pathlib import Path
import plistlib
import platform
import re
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
PINNED = {"torch": "2.8.0", "torchaudio": "2.8.0", "numpy": "1.26.4", "scipy": "1.16.2",
          "soundfile": "0.13.1", "pyinstaller": "6.16.0"}
BACKEND_MODULES = ("model_identity", "model_runtime", "model_review", "model_service", "model_inference")
IGNORED = shutil.ignore_patterns(".git", ".venv*", "__pycache__", "*.pyc", "outputs", "checkpoints")


def digest(path):
    value = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(8 * 1024 * 1024), b""):
            value.update(chunk)
    return value.hexdigest()


def safe_tree(root):
    for path in root.rglob("*"):
        if path.is_symlink() and (not path.exists() or not path.resolve().is_relative_to(root.resolve())):
            raise ValueError(f"Symlink escapes resources or is broken: {path}")


def required_file(path):
    if not path.is_file():
        raise ValueError(f"Missing required input: {path}")


def mounted_volume(path):
    mounts = subprocess.check_output(["/sbin/mount"], text=True)
    for line in mounts.splitlines():
        match = re.match(r"(/dev/\S+) on (.+) \(([^,]+),", line)
        if match:
            root = Path(match[2])
            if root.parent == Path("/Volumes") and path.is_relative_to(root):
                return root, match[3]
    raise ValueError("Output and work directories require a mounted external APFS volume")


def external_storage(path, require_apfs=True, seen=()):
    root, filesystem = mounted_volume(path)
    if root in seen:
        raise ValueError("External disk-image backing chain contains a cycle")
    if require_apfs and filesystem != "apfs":
        raise ValueError("External build volume must use APFS to preserve native library symlinks")
    # A mounted disk image is acceptable only when its backing file is on an
    # independently mounted external volume, not merely a directory named /Volumes.
    images = plistlib.loads(subprocess.check_output(["/usr/bin/hdiutil", "info", "-plist"]))
    for image in images.get("images", []):
        if any(entity.get("mount-point") == str(root) for entity in image.get("system-entities", [])):
            backing = Path(image["image-path"]).resolve()
            backing_root, _ = mounted_volume(backing)
            if backing_root != root:
                external_storage(backing, require_apfs=False, seen=(*seen, root))
                return root
    info = subprocess.run(["/usr/sbin/diskutil", "info", "-plist", str(root)], capture_output=True)
    if info.returncode == 0 and plistlib.loads(info.stdout).get("Internal") is False:
        return root
    raise ValueError("Cannot verify that the mounted external volume is externally backed")


def make_plan(args, checkpoint):
    if sys.platform != "darwin" or platform.machine() != "arm64" or sys.version_info[:2] != (3, 12):
        raise ValueError("Build with native macOS arm64 Python 3.12; cross-compilation is unsupported")
    backend_commit = subprocess.check_output(["git", "-C", str(args.backend), "rev-parse", "HEAD"], text=True).strip()
    expected = json.loads((ROOT / "integrations/apex-ai/backend.json").read_text())["reference_revision"]
    if backend_commit != expected:
        raise ValueError(f"Backend revision must match backend.json: {expected}; found {backend_commit}")
    if subprocess.check_output(["git", "-C", str(args.backend), "status", "--porcelain"], text=True).strip():
        raise ValueError("Backend checkout must be clean for a reproducible build")
    versions = {}
    for name, expected in PINNED.items():
        try:
            actual = metadata.version(name)
        except metadata.PackageNotFoundError:
            raise ValueError(f"Missing build dependency: {name}=={expected}") from None
        if actual != expected:
            raise ValueError(f"Build requires {name}=={expected}; found {actual}")
        versions[name] = actual
    for module in ("PyInstaller", "torch", "torchaudio", "soundfile", "scipy", "numpy"):
        if util.find_spec(module) is None:
            raise ValueError(f"Build dependency is not importable: {module}")
    media = {}
    for tool in (args.ffmpeg, args.ffprobe):
        version = subprocess.check_output([str(tool), "-version"], text=True).splitlines()[0]
        architecture = subprocess.check_output(["/usr/bin/lipo", "-archs", str(tool)], text=True).strip()
        if "arm64" not in architecture.split():
            raise ValueError(f"Media binary does not contain arm64: {tool}")
        media[tool.name] = {"version": version, "architecture": architecture, "sha256": digest(tool)}
    if args.output.is_relative_to(args.work_dir) or args.work_dir.is_relative_to(args.output):
        raise ValueError("Output and work directories must not contain one another")
    # Account for native wheels, their frozen copy and analysis intermediates.
    dependency_bytes = sum(Path(dist.locate_file(file)).stat().st_size
                           for name in PINNED for dist in (metadata.distribution(name),)
                           for file in dist.files or [] if Path(dist.locate_file(file)).is_file())
    minimum = max(4 * 1024**3, (checkpoint.stat().st_size + dependency_bytes) * 4)
    for path in (args.output, args.work_dir):
        parent = path.parent
        while not parent.exists():
            parent = parent.parent
        if not os.access(parent, os.W_OK):
            raise ValueError(f"External directory is not writable: {parent}")
        if shutil.disk_usage(parent).free < minimum:
            raise ValueError(f"Insufficient external free space: need at least {minimum} bytes")
    service = "service/filmcraft-ai"
    manifest = {"schema_version": 1, "platform": "macos-arm64", "service": service,
                "model_workspace": "model", "checkpoint": "model/checkpoints/BEATs_strong_1.pt",
                "ffmpeg": "service/_internal/bin/ffmpeg", "ffprobe": "service/_internal/bin/ffprobe"}
    manifest["required_files"] = [service, manifest["checkpoint"], manifest["ffmpeg"], manifest["ffprobe"],
        "model/scripts/infer_apex_audio.py", "model/PretrainedSED/config.py", "model/PretrainedSED/LICENSE",
        "service/_internal/backend/integrations/filmcraft/bridge.py", "service/_internal/model_inference.py"]
    command = [sys.executable, "-m", "PyInstaller", "--noconfirm", "--clean", "--onedir",
               "--name", "filmcraft-ai", "--contents-directory", "_internal",
               "--specpath", str(args.work_dir), "--workpath", str(args.work_dir / "pyinstaller"),
               "--distpath", str(args.work_dir / "dist"), "--paths", str(args.backend)]
    for module in ("torch", "torchaudio", "soundfile", "scipy", "numpy"):
        command += ["--collect-all", module]
    for name in BACKEND_MODULES:
        # Sources are data too: configuration_identity and inference metadata hash __file__.
        command += ["--hidden-import", name, "--add-data", f"{args.backend / (name + '.py')}:{'.'}"]
    command += ["--add-data", f"{args.backend / 'integrations/filmcraft/bridge.py'}:backend/integrations/filmcraft",
                "--add-binary", f"{args.ffmpeg}:bin", "--add-binary", f"{args.ffprobe}:bin",
                str(ROOT / "integrations/apex-ai/frozen_entry.py")]
    return {"status": "checked", "manifest": manifest, "packages": versions, "media": media,
            "backend_commit": backend_commit,
            "minimum_free_bytes": minimum, "command": command,
            "entry_available": (ROOT / "integrations/apex-ai/frozen_entry.py").is_file()}


def copy_licenses(destination):
    packages = {}
    for dist in metadata.distributions():
        name = dist.metadata.get("Name", "unknown")
        packages[name] = dist.version
        for file in dist.files or []:
            if ".." in file.parts or not any(word in str(file).lower() for word in ("license", "copying", "notice")):
                continue
            source = Path(dist.locate_file(file))
            if source.is_file() and source.suffix not in (".py", ".pyc", ".so", ".dylib"):
                target = destination / re.sub(r"[^a-zA-Z0-9._-]", "_", name) / str(file)
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(source, target)
    return packages


def verify_native_closure(root):
    """Fail packaging when native code still links to this machine's package manager."""
    for path in root.rglob("*"):
        if path.is_symlink() or not path.is_file():
            continue
        with path.open("rb") as stream:
            magic = stream.read(4)
        if magic not in (b"\xcf\xfa\xed\xfe", b"\xfe\xed\xfa\xcf", b"\xca\xfe\xba\xbe", b"\xbe\xba\xfe\xca"):
            continue
        linked = subprocess.check_output(["/usr/bin/otool", "-L", str(path)], text=True)
        for line in linked.splitlines()[1:]:
            dependency = line.strip().split(" (", 1)[0]
            if dependency.startswith("/") and not dependency.startswith(("/usr/lib/", "/System/Library/")):
                raise ValueError(f"Native dependency escapes bundle: {path}: {dependency}")


def build(args, plan, checkpoint):
    required_file(ROOT / "integrations/apex-ai/frozen_entry.py")
    # Exclusive creation protects another run between preflight and staging.
    args.work_dir.mkdir(parents=True, exist_ok=False)
    args.output.mkdir(parents=True, exist_ok=False)
    cache = args.work_dir / "cache"
    temporary = args.work_dir / "tmp"
    temporary.mkdir()
    environment = os.environ | {"PYINSTALLER_CONFIG_DIR": str(cache), "TMPDIR": str(temporary),
                                "PYTHONDONTWRITEBYTECODE": "1"}
    subprocess.run(plan["command"], env=environment, check=True)
    shutil.move(str(args.work_dir / "dist/filmcraft-ai"), str(args.output / "service"))
    model = args.output / "model"
    shutil.copytree(args.model_workspace / "PretrainedSED", model / "PretrainedSED", symlinks=True, ignore=IGNORED)
    (model / "scripts").mkdir()
    shutil.copy2(args.model_workspace / "scripts/infer_apex_audio.py", model / "scripts/infer_apex_audio.py")
    (model / "checkpoints").mkdir()
    shutil.copy2(checkpoint, model / "checkpoints/BEATs_strong_1.pt")
    for name in ("provenance.json", "README.md", "README.zh-CN.md"):
        source = args.model_workspace / name
        if source.is_file():
            shutil.copy2(source, model / name)
    licenses = args.output / "licenses"
    shutil.copytree(args.media_license_dir, licenses / "media-tools", symlinks=True)
    packages = copy_licenses(licenses / "python")
    safe_tree(args.output)
    verify_native_closure(args.output)
    for relative in plan["manifest"]["required_files"]:
        required_file(args.output / relative)
    manifest = plan["manifest"] | {"python": sys.version, "packages": packages,
        "build_dependencies": plan["packages"], "media": plan["media"], "backend_commit": plan["backend_commit"],
        "source_sha256": {name + ".py": digest(args.backend / (name + ".py")) for name in BACKEND_MODULES},
        "checkpoint_sha256": digest(checkpoint),
        "sha256": {str(path.relative_to(args.output)): digest(path) for path in sorted(args.output.rglob("*"))
                   if path.is_file() and not path.is_symlink()},
        "acceptance": "unverified; frozen staging does not establish relocated or clean-machine acceptance"}
    (args.output / "manifest.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    return {"status": "built", "output": str(args.output), "work_dir": str(args.work_dir),
            "size_bytes": sum(path.stat().st_size for path in args.output.rglob("*") if path.is_file() and not path.is_symlink())}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for option in ("backend", "model-workspace", "ffmpeg", "ffprobe", "media-license-dir", "output", "work-dir"):
        parser.add_argument("--" + option, type=Path, required=True)
    parser.add_argument("--check-only", action="store_true", help="Validate and print a build plan without writing files")
    args = parser.parse_args()
    try:
        for key, value in vars(args).items():
            if isinstance(value, Path):
                setattr(args, key, value.expanduser().resolve())
        for path in (args.output, args.work_dir):
            if path.exists() or path.is_symlink():
                raise ValueError(f"refusing to overwrite existing path: {path}")
        external_storage(args.output.expanduser().resolve())
        external_storage(args.work_dir.expanduser().resolve())
        checkpoint = args.model_workspace / "checkpoints/BEATs_strong_1.pt"
        required_file(checkpoint)
        required_file(args.model_workspace / "scripts/infer_apex_audio.py")
        required_file(args.model_workspace / "PretrainedSED/config.py")
        required_file(args.model_workspace / "PretrainedSED/LICENSE")
        for name in BACKEND_MODULES:
            required_file(args.backend / f"{name}.py")
        required_file(args.backend / "integrations/filmcraft/bridge.py")
        for tool in (args.ffmpeg, args.ffprobe):
            required_file(tool)
        if not args.media_license_dir.is_dir() or not any(args.media_license_dir.rglob("*")):
            raise ValueError("Missing media redistribution licenses and source/build provenance")
        safe_tree(args.model_workspace / "PretrainedSED")
        safe_tree(args.media_license_dir)
        plan = make_plan(args, checkpoint)
        result = plan if args.check_only else build(args, plan, checkpoint)
    except (ValueError, OSError, subprocess.SubprocessError) as error:
        parser.error(str(error))
    print(json.dumps(result, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
