# Validation status

Verified on Ryan's Linux workstation on 2026-10-18 with Rust 1.99.1.
The prior installation and execution blocker has been removed.

- `cargo fmt --all -- --check`: passed.
- `cargo test --workspace --all-features --locked`: 8 core workflow tests and
  1 desktop rendering test passed.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`:
  passed.
- `cargo build --workspace --all-features --locked`: passed.
- The compiled CLI created a disposable Git-backed demo and scanned its two
  pages, missing descriptions and broken Blog link correctly.
- The compiled native desktop application launched successfully and produced a
  built-in validation screenshot. The screenshot covers startup; the headless
  desktop test draws all six views.
- Original handoff assets remain unchanged. The catalog contains 118 entries in
  12 families; this count is checked against the bundled source.

Core tests exercise scanning, framework detection, source review, save, local
commit, undo, stale drafts, dirty repositories, credential patterns, failed-commit
rollback, demo overwrite protection and symlink boundaries. The rollback test
uses an empty Git author name: Git permits an empty email address.

The optional desktop feature `validation-screenshot` enables eframe's built-in
`EFRAME_SCREENSHOT_TO` capture. It is disabled in normal builds. Local toolchain,
demo and screenshot outputs live under ignored `data/`.

GitHub Actions checks Ubuntu and Windows on pushes and pull requests. Remote CI
results are separate from the Linux results above. Windows interactive behavior
and screen-reader accessibility still need platform testing.

This is a runnable starter. It has not passed the full production brief's Astro
acceptance flow. Structured page adapters, AI helpers, deployment and live rollback
remain roadmap items.

## Sidebar Video milestone 1 · 2026-10-19

Verified on Ryan's Linux workstation using the existing project-local Rust
1.99.1 toolchain and cached crates:

- `cargo fmt --all -- --check`: passed.
- `cargo test --workspace --all-features --locked --offline`: all 14 tests passed
  (8 existing Catalyst core, 1 existing desktop, 3 video core and 2 new native UI).
- `cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings`:
  passed.
- `cargo build --workspace --all-features --locked --offline`: passed.
- After the final minimum-window scrolling adjustment, the affected native UI
  tests, strict Clippy and locked build passed again.
- The real compiled `sidebar-video` application launched and exited successfully
  under Xvfb at 1480×1000 horizontal and its minimum 1160×800 vertical size.
  Both screenshots were inspected. Record, its explanatory text and the sequence
  actions fit above/below the stage as intended. The takes pane and nine-slot
  list scroll when the window cannot show their full contents.
- Headless egui checks draw both shapes with the bundled fonts, inspect native
  AccessKit labels, and require the Record control's accessible bounds to remain
  above the sequence at both supported sizes. Color tests require at least
  4.5:1 contrast for actual text/surface combinations.
- Video core checks exact output dimensions, shape changes preserving name and
  slots, nine stable bounded slot keys, and invalid/credential-like names leaving
  the previous name unchanged through the reused Catalyst secret check.
- Font/license SHA-256 values match the bundled provenance README. Cargo.lock
  adds only the two local packages; no third-party package was upgraded.

Native screenshots live in ignored
`data/sidebar-video-foundation/{landscape,minimum-portrait}.png`. Reproduce them
after the all-features build from this workspace root:

```sh
mkdir -p data/sidebar-video-foundation
EFRAME_SCREENSHOT_TO="$PWD/data/sidebar-video-foundation/landscape.png" \
  xvfb-run -a -s '-screen 0 1600x1200x24' target/debug/sidebar-video
SIDEBAR_VIDEO_VALIDATION_MIN=1 \
  EFRAME_SCREENSHOT_TO="$PWD/data/sidebar-video-foundation/minimum-portrait.png" \
  xvfb-run -a -s '-screen 0 1600x1200x24' target/debug/sidebar-video
```

Both capture behavior and the minimum-size/portrait validation environment
switch require the optional `validation-screenshot` feature. Normal builds ignore
that switch. These are application-development screenshot controls, not media
capture or project data.

No recording, inference, export, upload or project persistence is implemented by
this foundation. Real device timing/audio, timeline/save invariants, renderer
parity, model capability, OAuth/upload, installer and screen-reader acceptance
remain unverified later milestones. Ubuntu/Windows CI has not been run for these
new members yet.

## Sidebar Video milestone 2 core · 2026-10-19

Verified with the existing project-local Rust 1.99.1 toolchain and cached crates:

- `cargo fmt -p catalyst-core -p video-core -- --check`: passed.
- `cargo test -p catalyst-core -p video-core --locked --offline`: all **31** tests
  passed (3 shared local-helper tests, all 8 original Catalyst workflow tests,
  and 20 video model/timeline/store tests).
- `cargo clippy -p catalyst-core -p video-core --all-targets --locked --offline -- -D warnings`:
  passed.
- `git diff --check`: passed at the core checkpoint.

Regression evidence covers every supplied timeline invariant: moving retains
clip-relative cues; splitting remaps boundary/later cues and clamps crossing
durations; trimming remaps/clamps offsets and durations; removal retains source
files and cues; exact sequence-time sums; save/load equality; and old JSON
remaining valid when save publication is interrupted. It also checks grouped
atomic edits, failed/no-op edits, 100-step undo/redo bounds, redo invalidation,
identity high-water marks, absolute 30 fps frame nudges, exact second nudges,
global-playhead split, unknown fields, invalid identities/references/slots,
oversized/credential-like input, long source durations and integer overflow.

The local persistence tests create real disposable Git repositories. They verify
new-folder-only initialization, tracked metadata/ignored media and private data,
local version commits, no remotes, source-file preservation, two stale windows,
outside mirror changes, advisory lock exclusion, failed commit preserving saved
JSON, unknown secret-bearing files staying untracked/untouched, and preexisting
staging remaining untouched. Unix symlink/hardlink boundaries and private folder
permissions are exercised. A real child process exits abruptly after temporary
data fsync but before rename; its parent verifies the previous JSON bytes and
successful reacquisition of the OS-released save lock. Another test fails actual
project publication after mirror preparation, opens the old authoritative JSON,
then restores permission and retries successfully.

This is focused core evidence. The concurrently developed milestone 3 UI,
workspace-wide build and native interactions are verified separately by the
coordinator. No physical capture, inference, network, export or upload was run.
Windows link-count/locking behavior, platform CI and hardware/filesystem power
failure recovery have not been exercised on this workstation. These results do
not declare the full native recording application complete.

## Sidebar Video milestone 3 timeline UI · 2026-10-19

The egui studio now forwards project, timeline, shape and history actions to
video-core. The installed Dioxus recorder is still available during capture
migration. This checkpoint does not claim continuous capture or overlay playback.

Actually run in the Catalyst Solo workspace with its local Rust toolchain:

- `cargo test --workspace --all-features --locked --offline`:38 tests passed,
  including the original Catalyst workflows,20 video-core tests and6 studio tests.
- `cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings`:passed.
- `cargo build --workspace --all-features --locked --offline`:passed.
- After native layout corrections, the affected6 studio tests and native build
  passed again. Real Xvfb launches exited0 at1480×1000 and1160×800; generated
  clip metadata only, no device or private media opened. The minimum portrait
  screenshot was inspected after44px targets, selected-text contrast, full-width
  playhead and compact information rows were corrected.
- UI tests check the save-ack/newer-draft race, loading guards, unsaved-name
  navigation guard, sequence operations/history and AccessKit Record bounds.

Screenshot harness: build `sidebar-video --all-features`, set
`SIDEBAR_VIDEO_VALIDATION_TIMELINE=1` with `EFRAME_SCREENSHOT_TO`, optionally
`SIDEBAR_VIDEO_VALIDATION_MIN=1`, and launch under `xvfb-run` as above. Outputs
are ignored `data/sidebar-video-timeline/`. Normal builds do not inject fixtures.
Native folder-picker interaction, physical device capture, screen-reader use,
Windows compilation and the remaining specification milestones are unverified.

## Sidebar Video continuous capture core · milestone 4 · 2026-10-19

Verified on Linux with the existing project-local Rust 1.99.1 toolchain and
installed system FFmpeg/ffprobe. No physical camera or microphone was opened.

- `cargo test --workspace --all-features --locked --offline`: **50 passed**,
  two deliberately ignored generated-media integration tests. This includes all
  eight original Catalyst workflow tests and six existing native studio tests.
- `cargo build --workspace --all-features --locked --offline`: passed.
- `cargo clippy -p catalyst-core -p video-core --all-targets --all-features --locked --offline -- -D warnings`:
  passed.
- `STORYFOLD_CAPTURE_TEST=1 cargo test -p video-core --features capture-test --locked --offline capture::engine::tests::generated_ -- --ignored --nocapture --test-threads=1`:
  both generated integrations passed again after the final shared-lock fix.
- `cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings`:
  passed.
- `cargo fmt -p catalyst-core -p video-core -- --check` and `git diff --check`:
  passed. No generated source is enabled by a normal app build.

Pure regressions cover twenty hold gestures and their order, delayed command
consumption, the exact tap threshold, explicit stop/cancel, half-open frame ranges,
segment boundaries, pending/active pruning protection, unknown-file preservation,
device-ID boundaries, Linux query-structure ABI, source links/playlists/devices,
subprocess output limits, timeout/cancellation and the feature/environment gate.
The shared streaming publisher tests collisions, no overwrite and cancelled
temporary-file cleanup. A duplicated-descriptor regression proves that dropping a
save/capture lock explicitly releases ownership even while an inherited handle
remains open. This repaired a failure exposed by the full parallel test run.

The first real FFmpeg integration runs one continuous generated 1920×1080/30
session, verifies changing 1280×720 RGBA preview frames, queues hold and tap/long
takes in order, and checks the finalized MP4 decoded frame count against its
selected range. It checks H.264/AAC tracks, audio/video duration within one frame,
and byte-identical decoded interior video from copied segments. It also checks
immutable approved overlay snapshots, clamped cue duration, one-batch undo,
source-preserving import/cancellation, live-session recovery exclusion and
completed-take recovery. A second integration kills only the generated FFmpeg
source during an active long take, verifies explicit failure with retained ranges,
then recovers a valid partial MP4. Successful test projects are disposable.

This is a core implementation checkpoint. New egui capture UI integration/native
interaction, real-camera first-frame latency, twenty physical holds, real-device
unplug, microphone sync and ten-minute drift remain unverified. The non-Linux
unsupported stub has not been cross-compiled here. Local import supports video
containers; image import, inference, overlay rendering, final sequence export,
YouTube and packaging belong to separate work. These tests do not declare the
full studio or physical capture production-ready.

## Shared overlay renderer core · milestone 5 · 2026-10-19

Actually run with the project-local Rust 1.99.1 toolchain and cached dependencies:

- Initial `cargo test --workspace --all-features --locked --offline`: **59 passed**,
  two opt-in generated capture tests ignored. This preceded the coordinator's
  concurrent capture-UI integration and the final focused renderer refinements.
- `STORYFOLD_RENDERER_PROOF=1 cargo test -p video-core --all-features --locked --offline renderer::tests -- --nocapture --test-threads=2`:
  **nine renderer tests passed** after layout, motion and overlap refinements.
- `cargo clippy -p video-core --all-targets --all-features --locked --offline -- -D warnings`:
  passed. Initial workspace lint found constant-chunk style warnings, corrected
  using Rust 1.88-compatible array chunks. Final workspace verification belongs
  after the concurrently edited native UI compiles.
- Explicit generated capture suite rerun with `STORYFOLD_CAPTURE_TEST=1` and
  `--ignored --test-threads=1`: both integrations passed with the extended
  immutable content snapshots. No physical inputs were used.
- `cargo fmt -p video-core -- --check` and `git diff --check`: passed at the core
  checkpoint. The font and license SHA-256 hashes match the preserved provenance.
- The final focused Unicode test and strict core lint passed again after adding
  combining-mark/ligature cluster expansion, so a scalar selection inside one
  shaped glyph remains visibly emphasized.

Renderer checks cover all six types in both output shapes, all nine slots,
readable text size/safe bounds, overflow errors, unsupported glyphs, Unicode
scalar-to-wrapped-line highlights, static/reduced motion, all four emphasis modes,
600ms/90ms timing, transparent half-open intervals, deterministic two-font-only
output, bounded overlap, cached layouts, and straight-alpha bytes. They also
verify that slot2 fired4.0s into take3 still renders the identical snapshot at4.0s
when take3 moves first, despite a later slot edit. Removed and inactive cues do
not render; active legacy kind-only cues report missing content. Asset checks
cover explicit preparation, source removal after caching (no per-frame read),
missing images, and Unix symlink/hardlink rejection.

Generated-only proof images were produced under
`/tmp/storyfold-overlay-proof-GbowwI/`: the contact sheet
`six-types-both-shapes.png` and twelve full-size frames. The contact sheet and a
full horizontal screenshot card were visually inspected; readable text and full
contained images fit their cards. These are renderer PNG proofs, not screenshots
of a completed capture/playback integration. Reproduce with the test command
above; it prints a new disposable proof directory. Normal tests/app runs do not
create those proof files.

No actual FFmpeg overlay/export parity, native overlay playback, physical capture,
Windows/platform CI, screen-reader use or production frame-rate measurement was
performed for this milestone. AI review, image-import UI, inserts, sequence export,
YouTube and packaging remain separate work. A shared raw RGBA path is implemented;
that alone is not evidence of encoded-video parity or a complete application.

### Cached overlay frame cost · 2026-10-19

An opt-in generated benchmark prepares images, fonts, layouts and glyphs before
measuring30 animated1920×1080 frames with one screenshot card and two text cards.
The timed loop performs no file reads. Run the ignored
`renderer::tests::cached_1080p_three_card_timing` test with `--nocapture` in both
normal and `--release` profiles. On this workstation, one run measured:

| Profile | Mean | Median | p95 | Maximum | Throughput |
| --- | ---: | ---: | ---: | ---: | ---: |
| Debug |2717.231ms|2712.232ms|2842.515ms|2850.814ms|0.37fps|
| Release |70.656ms|70.932ms|79.057ms|83.414ms|14.15fps|

Both explicit benchmark tests passed; these numbers are evidence of cost, not a
performance acceptance pass. Even release exceeds the33.33ms budget for30fps
native overlays. Keep the preview worker/queue bounded, report this limitation,
and measure a scoped optimization before promising smooth30fps card animation.
No performance optimization was included in the renderer correctness checkpoint.
The benchmark remains ignored by ordinary test runs.
