# Standalone AI runtime build (macOS arm64)

Run `build_runtime.py` with native Python 3.12 and the versions in
`requirements-build.txt`. Use an isolated external build environment or an
external `--target` tools directory on `PYTHONPATH`; the command never installs
packages, downloads weights, or changes the source model environment.

```sh
python packaging/ai/build_runtime.py \
  --backend /path/to/clean/apex-fps-mvp \
  --model-workspace /path/to/apex-voice-detect \
  --ffmpeg /path/to/ffmpeg --ffprobe /path/to/ffprobe \
  --media-license-dir /path/to/media-licenses-and-provenance \
  --output /Volumes/ExternalAPFS/FilmCraft/task/dist/ai-runtime \
  --work-dir /Volumes/ExternalAPFS/FilmCraft/task/build-runtime \
  --check-only
```

Both output paths must be absent. The command refuses overwrites and local
output, checks free space and writable parents, and verifies an actual external
APFS mount. An APFS sparse image is supported when `hdiutil` identifies its
backing file and `diskutil` identifies the backing physical volume as external.
It does not create a missing mount directory. Disk metadata and external writes
need normal OS/sandbox access. `--check-only` prints the command, dependency
versions and runtime contract as JSON without creating output; `entry_available`
reports whether the frozen entry source has been integrated. Remove
`--check-only` to build after integration.

The backend must match the revision in `integrations/apex-ai/backend.json` and
have a clean working tree. The model workspace must contain the loader,
PretrainedSED (including its LICENSE), and the single checkpoint
`checkpoints/BEATs_strong_1.pt`. Supply media tool licenses and source/build
provenance for the complete native dependency closure. The supplied tools must
contain arm64. PyInstaller collects both media binaries, their native library
closure, backend modules, NumPy/SciPy/SoundFile/Torch/Torchaudio; raw backend
source files are retained for content identities. The output has the exact
`manifest.json` paths in [the spec](../../docs/ai-selection/standalone-runtime.md),
model resources, Python distribution licenses, and media provenance. The
manifest records versions, backend commit, tool/checkpoint digests and every
regular output file digest. Symlinks must resolve within resources; native
libraries may reference bundled or macOS system dependencies only.

On failure, partially generated directories remain for inspection and cannot
be overwritten by a retry. Choose fresh output/work paths after diagnosing the
failure. Build staging alone does not verify real inference, relocation,
cancellation, native launch, another Mac, signing, or notarization; these are S3
acceptance steps. The macOS package script copies runtime resources before code
signing. Signing changes native file bytes, so manifest digests describe the
staged runtime before final application signing.

Public command tests use only stdlib `unittest`:

```sh
python -m unittest discover -s packaging/ai/tests -v
```

To exercise external-storage rejection and native read-only preflight, set
`FILMCRAFT_AI_TEST_VOLUME` to a mounted external APFS root, and
`FILMCRAFT_AI_BUILD_INPUTS` to a JSON mapping with the seven required CLI
arguments (without `--`). Use fresh paths. These optional tests create no model
or runtime output and require external metadata access. The ordinary tests
exercise local-storage rejection and existing-output protection without a model
environment.

S2 red/green evidence: the public CLI first failed each local-storage,
no-overwrite, missing-checkpoint, and read-only contract/source-pin slice; after
implementation all four passed with native dependencies and external metadata
access on 2026-10-18. Tests never ran PyInstaller or wrote model weights; real
packaging acceptance is recorded separately by S3.
