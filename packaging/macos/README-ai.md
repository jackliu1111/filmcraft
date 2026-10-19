# Package the macOS arm64 AI application

The current local delivery, final image hashes and real inference/native acceptance
are recorded in [standalone runtime acceptance](../../docs/ai-selection/implementation/standalone-runtime.md).

Build the frozen runtime with [the runtime command](../ai/README.md), then use
native arm64 FilmCraft and CLI release binaries:

```sh
CARGO_TARGET_DIR=/Volumes/ExternalAPFS/FilmCraft/task/target \
DIST=/Volumes/ExternalAPFS/FilmCraft/task/dist \
packaging/macos/package.sh --arch aarch64 \
  --ai-runtime /Volumes/ExternalAPFS/FilmCraft/task/runtime/ai-runtime
```

Add `--skip-build` when those target binaries already exist. The runtime may
also be supplied through `FILMCRAFT_AI_RUNTIME`; the command-line flag takes
priority. AI packaging requires existing absolute runtime input and external
APFS output locations. It verifies the manifest contract, required files,
contained symlinks, arm64 native code, physical external backing, writable
parents, and estimated free space before building or removing the previous
package work directory. It rejects x86_64 and universal AI apps. The package
flow without an AI runtime keeps the existing universal default. AI preflight
reads every embedded arm64 Mach-O deployment command and sets the app
`LSMinimumSystemVersion` to their highest macOS requirement (at least 11.0).
It does not infer support from the host version or SDK alone. Inspect an
existing runtime without building, modifying it or importing model code:

```sh
python3 packaging/macos/ai-package.py inspect /absolute/path/to/ai-runtime
```

Runtime resources are copied to `FilmCraft.app/Contents/Resources/ai-runtime`
before signing. The native editor is `Contents/MacOS/FilmCraft-core`; the
`CFBundleExecutable` remains `FilmCraft` and starts the packaged shell launcher.
Every real Mach-O is signed first, followed by enclosing framework/helper
bundles, native core, launcher and app. Final verification uses
`codesign --verify --strict --deep`; final signing does not use `--deep`.
Only three process executables receive `ai-runtime-entitlements.plist`'s
`disable-library-validation` exception: the frozen AI service and bundled
FFmpeg/ffprobe. Their bundled Python/native extensions and media libraries have
ad-hoc signatures without a shared Team ID. The editor, app and individual
libraries retain their existing entitlements. After strict verification,
packaging runs the signed service's `--check-runtime` with external
`<target>/macos-package/ai-signing-smoke` data, then both bundled media tools'
`-version` commands before producing the DMG. Any loader failure aborts packaging
even when static signature verification succeeds; these checks do not import
the model or process media.
Manifest digests describe the staged runtime before code signing changes native
bytes. The existing signing/notarization environment variables and defaults
still apply; default ad-hoc signatures are for local testing. Developer ID
notarization and a separate clean Mac require their own validation.

AI DMGs use `hdiutil create -srcfolder -fs APFS` to preserve signed native
resources. The previous HFS `makehybrid` flow can insert FinderInfo into native
executables, causing strict app verification to fail only after the image is
mounted. After creating the compressed DMG, packaging attaches that final image
read-only, verifies its `FilmCraft.app` with `codesign --verify --strict --deep`,
and detaches it even when verification fails. A failed final-image check stops
distribution. hdiutil temporary copies use the already verified external output
directory. The public `ai-package.py image` command independently checks that its
existing output parent is on a physically verified external APFS volume, is
writable, and has estimated room for APFS overhead, temporary copies and the
compressed image before invoking hdiutil. Output must be a new absolute `.dmg`
path outside the source stage. Non-AI packaging retains its existing CI image
creation flow.

On first launch from Finder, choose an existing folder on a mounted external
APFS or HFS (Mac OS Extended) volume for AI data. An APFS disk image backed by
an external disk is supported; its physical backing disk may use ExFAT. The
filesystem requirement applies to the selected data mount, and physical external
backing is verified independently. Direct ExFAT data is rejected before cache/tmp
creation or backend startup: macOS AppleDouble `._*.json` files can be mistaken
for task JSON by the current shared backend. Use a compatible mounted volume or
APFS image instead; the application does not fall back to local data storage.
The frozen service's `--check-runtime` validates the runtime
and external storage without importing the model. The launcher remembers only
the literal path in a small per-user setting at
`~/Library/Application Support/FilmCraft/ai-data-directory`. If the remembered
volume is missing or invalid, reconnect it and select the folder again, or
cancel. A damaged runtime also stops startup; reinstall the complete app.

Explicit `APEX_AI_DATA_DIR` or `FILMCRAFT_AI_DATA_DIR` skips the chooser and is
not remembered. `APEX_AI_DATA_DIR` has priority. An invalid explicit location
stops startup with an error and does not switch to another directory. Paths
with spaces, Unicode, shell punctuation and newlines are passed literally.
After validation, native data uses `<selected>/filmcraft`; model caches and all
temporary paths use subdirectories of the selected folder. The bundle remains
read-only. `--help` and `--version` work without selecting or mounting data.
Data paths inside any part of the enclosing `.app` are rejected before cache
or temporary directories are created, including runtime ancestors and sibling
resources. Explicit `FILMCRAFT_APEX_PYTHON` or `FILMCRAFT_APEX_BRIDGE` selects
the native developer entry before frozen-runtime preflight. This path retains
the caller's data/cache/native options and does not require the bundled service.
The normal headless CLI zip remains separate from the app's AI resources.

Public command/launcher tests:

```sh
python3 -m unittest discover -s packaging/macos/tests -v
bash -n packaging/macos/package.sh packaging/macos/ai-launcher.sh
```

Set `FILMCRAFT_AI_TEST_VOLUME` to a verified, writable external APFS directory
to include the real tiny signed-app image regression. It builds no model or
editor: it creates/mounts an APFS DMG, verifies the signed fixture, and confirms
that changed resources make the final-image gate fail. The test requires normal
disk-image mount access. Without this explicit directory it is skipped.

TDD evidence (2026-10-18): the public package test first failed because
`--ai-runtime` was unknown; after preflight integration it passed and verified
that existing work and a nonexistent distribution directory remain untouched
on invalid input. The relocated Unicode/space app fixture then failed because
the launcher did not exist; after implementation it passed using external
service/native process fixtures. Regression tests also cover explicit failure
stopping native startup, literal remembered paths without terminal environment,
argument preservation, override precedence, informational commands without data,
and rejecting unsupported AI architectures. These small fixtures do not run
model inference, mount a real external volume, invoke a folder dialog, or prove
native code signing. A separate public inspector test compiles tiny real
arm64 executables targeting macOS 11.0 and 27.0, first failed because the
inspector was absent, then passed with the reported maximum 27.0. Read-only
inspection of the staged runtime on 2026-10-18 found 240 native files and a
27.0 minimum (the supplied FFmpeg build requires macOS 27.0). Actual
external-volume packaging, relocation, inference,
cancellation and native launch are S3 acceptance work, recorded separately.

Review-fix TDD (2026-10-18): first reproduced writes from startup/probe into
runtime ancestors within a real `.app` layout, then verified startup/probe/
analyze/validate all reject internal data locations without changing the bundle
tree. Developer overrides first failed on a missing frozen service; they now
reach the native process with arguments and explicit options intact, while
ordinary packaged startup still rejects the missing service. The public signing
smoke CLI's controlled loader-failure test first failed because the command was
absent, then passed with the error propagated to the packaging caller. These
process fixtures are separate from actual signing and model acceptance.

The relocated DMG later exposed the same Team ID failure in FFmpeg/ffprobe.
The smoke fixture first returned success despite either media executable failing,
then passed after both tools were added to startup checks. The fixture also
verifies all three processes start with their expected public arguments when
healthy. The signing exception now covers exactly those three executables.
