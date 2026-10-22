# Validation status

## Native Record studio 0.1.9 — 2026-10-21

Installed Linux Preview sequence plays current source trims and audio directly,
with clock-driven cards, photos, video inserts and camera-card layouts. It opens
an immutable edit snapshot without first rendering an MP4, saving the project or
creating a version. Press Preview again after editing. Explicit Export MP4 keeps
its rendering progress and local-version behavior. An active take must finish
saving before preview opens. Other platforms retain rendered compatibility preview.

The workspace passed 118 video-core, 57 studio and 17 other ordinary tests,
formatting and strict all-target/all-feature Clippy. Three independent preview
regressions each passed 59 repeated runs. Generated native tests checked both
orientations against the shared scene renderer, source audio decoding, actual
Play/Pause/fullscreen/Escape/replay controls, normal player closing, and
preview/export/cancel behavior with newer drafts preserved. Actual Record and
Preview clicks passed at both supported window sizes without an MP4 export or
project save from Preview. Audio used a null sink; no physical devices opened.

Normal-feature release, isolated installer checks, extracted package hashes,
libraries, desktop launcher and eight-second isolated startup passed. Installed
hashes match `storyfold-record_0.1.9~preview.1_amd64.deb`. Machine-readable
checks and generated startup timings are in the parent project's
`docs/validation/record-studio-0.1.9.json`.

Direct playback removes the full-sequence render wait; source validation, player
startup and take finalization still take time. Dynamic composition still uses
CPU work. Physical audible quality, lip sync, camera latency, OBSBOT connection,
app unification and other-platform acceptance remain separate checks.

## Native Record studio 0.1.8 — 2026-10-21

Installed 0.1.8 prefers NVIDIA NVENC for recording and sequence encoding when a
bounded background initialization check succeeds. Project exposes a CPU/GPU
preference for the next job. Progress names the selected backend. Interrupted
recordings retain that backend for recovery; older sessions default to CPU.
Changing the preference invalidates the prepared preview cache.

The native workspace passed 105 video-core, 53 studio and 17 other tests, plus
formatting and strict all-target/all-feature Clippy. Actual NVENC generated checks
passed 1080p capture/recovery with exact frames, copied interior frames, audio and
unchanged sources, and both-shape exports with cuts, audio, overlays and local
versions. Generated cancellation/draft preservation and actual isolated native
Record and Play/Pause/fullscreen/replay controls passed. AAC decoding used a
test-only null sink. Normal-feature release, installer fixtures, package extraction,
libraries, launcher and eight-second isolated startup passed. Installed hashes
match `storyfold-record_0.1.8~preview.1_amd64.deb`.

Audio, decoding, scaling and composition still use CPU work. The generated encode
benchmark excludes camera/USB/display latency; physical smoothness and audible
quality remain unverified. This adds NVIDIA encoding only. Preflight can choose
CPU; later GPU failures remain errors with the existing source/recovery behavior.
OBSBOT discovery and distorted audio remain separate investigations.

## Native Record studio0.1.7 — 2026-10-21

Installed0.1.7 restores microphone discovery on this Linux workstation by adding
`pulseaudio-utils`, now also a package dependency. The system-preferred detected
input is selected before capture is enabled. An explicit Off remains off through
refreshes; the preview badge names the selected input or warns of silent takes.
No hardware opens until Enable. Existing silent sources remain unchanged.

Clips show local audio envelopes decoded by a single background worker with
bounded memory. Trims, splits, reordered clips, zoom and scrolling map to original
source time. Loading, silent, missing-track and error states are distinct.
Error retry is explicit. Analysis preserves project JSON and source bytes.

Generated tone survives capture, take cutting, trimmed/joined MP4 exports and
native48kHz stereo AAC decoding. The player test uses a null audio sink and
checks unmuted playback plus actual Play/Pause/fullscreen/replay controls.
Waveform tests cover real extraction and the app worker/cache/drawing path.
The complete native workspace suite passed; after final layout corrections,
51 studio tests, workspace warning-denied Clippy and formatting passed again.
The unchanged video core passed96 ordinary tests. Generated native Record
controls pass at1480×1000 and1160×800; camera setup/footer bounds are also tested.

Normal-feature release, isolated installer checks, extracted package hashes,
dependencies, libraries, desktop launcher and startup passed. The managed user
installation was atomically updated and opened with devices off.
Package: `storyfold-record_0.1.7~preview.1_amd64.deb`.

The affected original recording was not supplied. No physical microphone/camera,
audible speaker output or lip sync was tested. Absent recorded audio cannot be
reconstructed. Source scrubbing remains silent; Preview sequence plays audio.
See `docs/validation/record-studio-0.1.7.json` in the parent project for evidence
and remaining platform/hardware limits.

## Native Record studio0.1.6 — 2026-10-21

Installed0.1.6 adds direct source trim grips, draggable clip units with an insertion
marker, zoom/focus, ruler seeking, cutting and local source-frame previews. Each
completed drag uses one core history step; source media and cue timing are
preserved. The bottom editing actions fit at both supported window sizes.

The live camera now requests repaint when a frame arrives; a slower fallback
checks status after frames stop. Three generated measurements isolate a median
8–9ms polling wait avoided by notifications. This is not sensor-to-screen timing
and does not establish that the remaining physical camera delay is solved.

39 studio and92 video-core unit tests, six timeline pointer checks, strict
workspace lint, formatting and the normal-feature release pass. Eleven generated
integration jobs cover recording/recovery, source scrubbing, sequence playback,
MP4 encoding/cancellation and native input. The final layout was inspected and
Record was rechecked at1480×1000 and1160×800. The final Debian package was
extracted, checked and started; the verified managed user installation was
atomically updated and opened.

Package: `storyfold-record_0.1.6~preview.2_amd64.deb`. Physical camera latency,
OBSBOT, fresh OS/Windows and real Google acceptance remain open. See
`docs/validation/record-studio-0.1.6.json` in the parent Storyfold project.

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

### Bounded renderer performance correction · 2026-10-19

The same generated30-frame/three-card/1920×1080 release benchmark measured:

| Implementation | Mean | Median | p95 | Maximum | Throughput |
| --- | ---: | ---: | ---: | ---: | ---: |
| Card-bound composition only |29.775ms|29.458ms|36.564ms|37.734ms|33.58fps|
| Card bounds + exact validated-layout reuse |23.052ms|22.500ms|30.329ms|34.770ms|43.38fps|

The isolated unchanged-content validation loop measured6.286ms per three-card
frame. Cache reuse does not remove validation: a new regression mutates a cached
snapshot with credential-like text, an invalid highlight range and a control
character; each fails and the original cache entry remains the sole valid entry.
The focused regression and strict all-target/all-feature core Clippy passed.

The release median and p95 meet the representative33.33ms target. The maximum
sample still exceeded it; nine simultaneous cards, physical capture interaction,
other hardware/platforms and sustained performance have not been measured.
No camera frame-rate change, image downsampling or layout simplification was made.

Final performance-checkpoint checks passed: release video-core suite **41 passed,
4 opt-in tests ignored**; strict all-target/all-feature core Clippy; formatting;
and whitespace checks. The explicit release old-path/reference proof passed for
every combination of six overlay types, four highlight styles, both shapes and
four animation times (192 output-frame comparisons), with exact straight-RGBA
byte equality. Its full-frame reference switch exists only under `cfg(test)`.
The previously recorded timing uses the same final rendering implementation;
expanding the equivalence matrix changed only the ignored proof harness.


## Native capture/card controls — 2026-10-19 (Edmonton)

- App8 unit/UI tests passed: both-shape AccessKit/control bounds, core history,
  draft/save acknowledgement, manual approved cue forwarding and stale renderer
  frame rejection. A boundary regression caught eager unsigned subtraction in
  initial UI cue lookup; the final implementation delegates lookup/clamping to
  typed core Edit::AddCueAtPlayhead.
- Opt-in `cargo test -p sidebar-video --features capture-test --locked --offline
  generated_pointer_space -- --ignored --nocapture` with
  `STORYFOLD_CAPTURE_TEST=1`: passed,9.43seconds. Generated video only: pointer
  hold released outside Record, tap-tap long take, mixed Space/pointer input,
  exactly3 completed clips, camera stop, newer name retained through finalize,
  save/load equality and recovery without duplicate clips. No hardware opened.
- Image import check passed: generated PNG bytes preserved; invalid image,
  cancelled input, symlink and hard-link imports published no additional file.
- App all-target/all-feature warning-denied Clippy passed. Native validation
  launches under Xvfb at1480×1000 horizontal and1160×800 portrait exited cleanly;
  both screenshots were inspected in data/sidebar-video-ui/. These are empty
  camera layouts, not evidence of native physical-camera operation.

The installed Dioxus application remains the usable release. Native sequence
playback/export integration, local AI, inserts, publishing/keychain, packaging
and physical timing/drift/unplug acceptance remain incomplete.

## Sequence MP4 export — 2026-10-19 (Edmonton)

- Release video-core all-feature suite: **47 passed,6 opt-in tests ignored**.
  Both all-feature and default-feature all-target warning-denied Clippy passed.
  Export sources passed formatting and whitespace checks. The coordinator owns
  final workspace/app checks; a newly added playback module was not part of this
  export checkpoint's formatting scope.
- Explicit generated-only release integration command, with
  `STORYFOLD_CAPTURE_TEST=1`:
  `cargo test --release -p video-core --all-features --locked --offline
  export::tests::generated_ -- --ignored --nocapture --test-threads=1`.
  Both tests passed, including a repeated run after enabling automatic RF64 for
  intermediate WAV files. Test folders include spaces. No hardware, network,
  account or upload action occurred.
- Both1920×1080 and1080×1920 generated exports contained24 frames at30fps,
  H.264 video and AAC48kHz stereo. Trimmed red/silent then blue/tone sources kept
  sequence order and expected audio. Decoded RGB difference from the shared
  renderer within a visible overlay was2.177 horizontal and1.058 vertical mean
  absolute channel values, under the explicit6-value tolerance. Vertical used
  animated cards, horizontal reduced motion. Samples outside the cue interval
  contained no overlay. These are specific lossy-encode parity checks, not proof
  for every camera, overlay combination or platform.
- Actual FFmpeg progress changed and stayed monotonic. Mid-encode cancellation
  left no final MP4 or new project version. Existing names, a destination created
  during encoding and a symlink destination were preserved/refused. Generated
  sources stayed unchanged. Successful exports saved a local Git version;
  deliberately broken Git identity retained the valid MP4 and saved JSON with
  a separate version error. An external save during export retained the newer
  JSON and completed MP4, reporting that the submitted snapshot was not saved.
- Pure regressions cover absolute frame rounding, zero-frame clip rejection,
  actual rounded vertical warning boundaries, timestamp overflow, missing legacy
  approved content, confined names, bounded progress parsing and cleanup of only
  registered private workfiles while retaining unknown content.

The baseline uses private temporary disk and a second H.264 encode. RF64 support
was exercised with short fixtures; no multi-hour/over4GiB fixture was generated.
No physical timing/drift, native export/player interaction, Windows CI, M7 inserts,
YouTube or packaging acceptance is claimed here. Shared atomic publication's
documented post-rename flush/power-loss caveats still apply. Root app integration
and subsequent milestone verification remain separate from this core checkpoint.


## Native preview/export controls — 2026-10-20 (Edmonton)

- App all-feature unit/UI suite: **11 passed,2 opt-in generated checks ignored**.
  New checks verify that export acknowledgement retains newer edits, a changed
  reduced-motion setting prevents stale automatic playback, and worker loss
  clears loading without falsely acknowledging a save.
- Explicit `generated_sequence_controls_cache_cancel_and_preserve_newer_edits`
  under Xvfb passed in10.61seconds. The test generated local H.264/AAC sources,
  rendered a two-clip sequence with a quote, opened the actual muted FFplay window,
  reused an unchanged preview, rebuilt after a trim, and canceled without leaving
  a final MP4. Source bytes and edits made during export were preserved. No
  physical camera, microphone, private recording, network or account was used.
- Native playback path checks reject URLs, traversal, missing files and links
  before launching a player. Rendering, player startup and folder opening stay
  off paint; progress comes from the core encoder rather than a timer.

This establishes generated native controller/player interaction. Physical camera
quality, timing/drift/unplug, local AI, M7 inserts, keychain publishing, packaging
and Windows acceptance remain separate. The installed Dioxus app is preserved.

## Visual inserts and zoom — 2026-10-20 (Edmonton)

- M7 pure regressions passed9/9: integer-millisecond/30fps clock over18,000 frames;
  bounded zoom centers/100–250%/positive duration, all three easing endpoints,
  reduced motion, box conversion, old JSON defaults, exact insert roundtrip and
  malformed/mixed content rejection; both shapes/all three scene layouts at0,
  2500 and5000ms; immutable cue after slot changes; missing/stale/overlapping
  insert errors; natural source thumbnail and no per-render file reads.
- Generated integrations passed3/3 in46.09s on the final pixel-safe renderer.
  Four six-second MP4s exercised horizontal fullscreen photo, vertical Beside
  photo, horizontal Behind video and vertical fullscreen video. Each had180
  frames at30fps, H.264/AAC and saved a local project version. All twelve decoded
  samples at0,2.5 and5s matched their shared live/timeline scene with RGB MAE
  **0.816–1.368**, below the explicit7-value lossy tolerance. Raw live/timeline
  frames were identical. Source camera media stayed unchanged.
- A24fps selected video became a muted30fps195-frame proxy; original bytes remained.
  Forward/backward exact-frame seeks matched, stale/end-of-source frames failed,
  cancellation published no extra proxy, and symlink/hardlink input was refused.
  Scene cancellation left no MP4 or saved JSON change. Overlapping inserts and
  source intervals extending past the last frame were refused before publication.
- Existing generated no-insert export regressions passed2/2 in13.05s after scene
  integration, including source order/audio/overlay timing, both shapes, actual
  progress/cancel, collisions, stale saves and local-version failure preservation.
- Generated proof is retained at `/tmp/storyfold-insert-proof-XWV9y1`: four MP4s
  under its project exports and24 expected/decoded PNGs. Portrait Beside and
  horizontal Behind decoded2500ms samples were visually inspected. The camera
  card is opaque with its original background; this is not person segmentation.
- Final exact optimization/reference proof passed36 full-HD comparisons across
  two shapes, three placements and six animation/zoom times, including translucent
  images and card entry. Identity card composition also matched pinned tiny-skia
  for every alpha/gray pair with both opaque and translucent destinations. Changed
  secret text and invalid zoom did not enter either bounded validation cache.

The cached generated1920×1080 scene benchmark uses an animated insert plus two
text cards and a1280×720 camera frame, with no IO in the measured30-frame loop:

| Implementation | Mean | Median | p95 | Maximum |
| --- | ---: | ---: | ---: | ---: |
| Initial scene |75.742ms|75.320ms|81.767ms|84.583ms|
| Final bounded composition |36.054ms|35.859ms|39.490ms|39.734ms|

A representative final profiled frame spent14.246ms sampling the insert,14.185ms
rasterizing cards,4.805ms drawing the camera card,1.575ms combining cards,1.780ms
producing straight RGBA and1.241ms setup. Exact-content validation reuse measured
0.002ms in that frame. These stages identify remaining work, not independent p95s.
The final scene still misses the33.33ms budget: **sustained30fps full-HD live scene
composition is not accepted**. Camera FPS, source quality and resampling were not
reduced. This does not measure physical input latency, ten-minute drift, unplug,
real VFR/HDR/rotation/platform behavior, or native app integration; coordinator
acceptance remains separate. Video inserts stay visual/muted while narration
continues, and Behind does not remove a background.

After container hardening, the final generated suite passed **4/4 in43.37s**.
A renamed ffconcat `.mp4` with a normal-name symlink reference was refused by
probe, import, canonical insert inspection/preparation and sequence export, with
no imported file or exported artifact. Controlled private concat lists continued
working in the parity cases. A320×240 source with2:1 pixel aspect became a
640×240 square-pixel proxy with six frames. Exported insert narration also passed
an audible RMS check (>0.02); video inserts themselves remain muted.


Final post-repair no-insert export regressions passed2/2 in13.70s. Strict
all-target/all-feature core Clippy, owned-source formatting and whitespace checks
passed. CPU/cgroup topology is queried once during renderer construction, keeping
frame composition free of topology/file queries. No physical device was opened.

Cue-relative clock follow-up: the focused insert suite passed5/5 in4.55s.
Its property loop checked54,003,000 mappings (all1000 source-in millisecond
residues × three rounded cue-start phases × every frame through600seconds),
plus large/u64-bound offsets and source-time overflow refusal. The generated
cursor/assets regression passed1/1 in3.66s:96 prepared source frames from four
source offsets and all three cue phases exactly matched independently decoded
proxy frames. At the6.5s proxy boundary6499ms selected valid frame194 and6500ms
was refused;5999ms still maps to179. The corrected mapping fixes the66ms/two-
frame cue difference without relaxing the actual decoder frame-count bound.

## Local AI evidence/review checks — 2026-10-20 (Edmonton)

- Core AI6 unit checks passed: strict root/nested proposal fields, HTML escaping,
  local vision/cloud gate, public target refusal, RSS parse/dedup and private
  monthly usage accumulation. App AI3 checks passed: canceled late approval,
  HTML acknowledgement/newer drafts/worker failure and minimum1160×800 accessible
  review controls. The initial single-row action layout overflowed; the final
  two-row controls pass the bounds check.
- Explicit `STORYFOLD_AI_TEST=1 cargo test -p video-core --all-features --locked
  --offline ai::tests:: -- --ignored --nocapture --test-threads=1`: **2 passed**,39.75s.
  Generated local vision/OCR/proposal/review/HTML used installed qwen3.5:9b with
  2224input+123output tokens. JSON and media remained unchanged before approval;
  invented copied text was refused; HTML, local version and private usage passed.
  The second check obtained4real RSS results and a1440×900 isolated public-page
  Chromium screenshot whose OCR matched the current page; precanceled capture
  was refused. No private recording, credentials, paid API or physical device
  was used. These are actual tool/model checks, not a model capability assertion.
- An initial unconstrained proposal rephrased OCR and was correctly rejected.
  The final generation schema enumerates exact source passages while retaining
  independent validation. The public-page check initially expected an old text
  heading now drawn as a logo; it now checks actual captured documentation text.
- Current official protocol references: https://docs.ollama.com/api/chat,
  https://docs.ollama.com/capabilities/structured-outputs,
  https://developer.chrome.com/docs/automation-and-testing/headless and
  https://chromium.googlesource.com/chromium/src/+/HEAD/net/docs/proxy.md.
  Full native button interaction, final workspace checks and packaging are being
  verified separately; no physical/Windows/Google-upload acceptance is implied.

- Opt-in app `generated_review_buttons_roundtrip` passed23.40s. Actual egui
  pointer clicks exercised source import, local Rebuild, shared Preview, Approve
  and HTML export at1160×800. No approved slot/JSON/media changed before approval;
  a newer draft made during export remained intact/dirty and the generated
  source bytes were unchanged. This used the committed generated-text fixture,
  local Qwen and a temporary project; no physical device or private recording.


## Native preview controls and YouTube integration · 2026-10-20

The native player now uses system mpv with an always-visible controller. Controller mouse/button commands through local mpv IPC on a generated MP4
verified Play/Pause, a stopped clock while paused,
fullscreen enter/exit, Escape, staying open at EOF and Play restarting the clip.
A compiled-in local Lua handler fixes replay after EOF. User configs/scripts,
network extraction and sidecar discovery remain disabled. Source bytes stayed
unchanged. No physical camera or microphone was opened by these checks.

The app queues camera stop and take saving before Preview/Export, with visible
status and cancellation. Actual egui pointer capture/preview and output/cache/
cancel/newer-draft integration tests both passed with mpv in27.20seconds.

The generated capture clock benchmark under another1080p encode measured109ms
median/132ms p95 across24samples from filter clock to latest native RGBA. This
excludes sensor/USB/driver and GPU painting. Camera video queues are2packets and
Pulse audio is8packets; final capture remains1920×1080 at30fps. Physical C920/
OBSBOT sensor-to-screen latency and ten-minute A/V drift remain unverified.

Project → YouTube integrates the worker protocol, native connection/upload/import
forms and existing append/autosave path. Credentials stay in the OS keychain;
opening the panel performs no credential access. Review gates bind the chosen
connection, metadata and rendered MP4. Google setup/consent and real publishing
remain user acceptance; no live account or private keychain was accessed.

Final locked offline workspace checks passed123tests (83video core,23studio,
8shared local,8existing workflow,1Catalyst UI),22opt-in checks ignored. All-target/
all-feature Clippy with warnings denied passed. Seven YouTube-panel fixtures cover
idle startup, review, real pointer confirmation, stale results, newer drafts,
worker failure and late cancellation. The modal pointer fixture first settles
egui layout before reading its accessibility bounds. Formatting and isolated
package-installer self-checks passed. See docs/YOUTUBE.md for protocol evidence.

Release compilation and installation of 0.1.1 are tracked separately below. No
Windows/macOS runtime, fresh-OS install, real Google upload or hardware timing
acceptance is inferred from these checks.

The host system yt-dlp2024.04.09 rejects two required isolation flags. Packaging
now bundles the pinned official2026.08.19 zip executable, verified against its
release SHA256 before execution, with unmodified LICENSE and THIRD_PARTY_LICENSES.
The tool passed help/version checks with plugin and remote-component loading
disabled. A confined app-prefix resolver prefers that bundle and rejects linked,
hardlinked, empty or non-executable existing tools. Two new local resolver
fixtures passed; all19YouTube tests passed after this change (two generated
integrations stayed opt-in). Full123-test workspace results above predate only
this resolver addition; final all-target/all-feature strict Clippy passed again.
Installer fixtures now also check bundled tools, their manifest hash, unmanaged
tool preservation and linked parent-directory refusal. No public clip was fetched.

Native0.1.1 was rebuilt without validation/capture-test features, packaged as
`storyfold-record_0.1.1~preview.2_amd64.deb`, extracted and started under Xvfb
for8seconds without stderr or missing libraries. Binary SHA256:
`2553b85a3a3bc578d8fdba8c487e122fad0d25e2fb41e7735a21b7913eb11884`.
User installation, manifest hashes and launcher validation passed. The installed
app was launched with version0.1.1in its title; older windows were left intact.
The bundled tool passed its restrictive help/version checks. Fresh-OS install,
physical timing, Windows/macOS and real Google/keychain acceptance remain open.


## Native startup/input follow-up0.1.2 · 2026-10-20

The user reports that0.1.1ignores all studio clicks. This remains an open real-
desktop acceptance failure. A validation-only, opt-in Input check logs at most100
count/state records without keys, paths, content or screenshots. It never forces
repaint. Its actual-desktop Project-click result is pending.

The new scripts/test-record-native-input.py launches only an owned app fixture
under fresh xvfb-run authority, waits for its viewable X11 window, focuses it and
sends real OS pointer events at its own measured Project bounds. The app receives
these events and opens Project. It passed with0.1.1and0.1.2; this is stronger than
injected egui-context events, but does not prove the actual GNOME desktop case.

Version0.1.2 also fixes confirmed startup UX problems: prominent Create/Open
actions, an inactive grey Record button and rfd project folder pickers parented
to the studio window. The two-shape/minimum-layout test verifies startup controls
fit above Record. All23studio tests and strict app Clippy passed; installer tests,
normal release build, package extraction/startup and installed hash verification
passed. No hardware, private project, keychain or Google action was performed.
Desktop input, actual picker foreground behavior and physical timing remain open.

## Record controls and physical startup · 2026-10-20

Ryan confirms Project opens and live camera video works; Record does nothing on
the desktop. This remains an open acceptance failure, not a reproduced core
failure. The generated egui capture/preview/recovery check passed again16.02s.
The native OS fixture now uses actual X11/XTest Record holds, release outside,
and taps on a generated live source at both1480×1000 and1160×800. Each saves two
clips. Version0.1.3 adds a standard Start recording / Stop & save action above
the video; actual OS clicks on it start and save a third clip at both sizes.
Recording errors now appear above the stage rather than below the controls.
25studio tests and strict app/core all-target/all-feature Clippy passed.

The bounded physical backend startup example opened OBSBOT Tiny2 and Logitech
C922 separately at1080p30 with microphone off, no take and no image viewing.
First preview was2019ms and601ms respectively; each reached sequence60, stopped
cleanly and deleted its temporary buffer. This measures startup, not end-to-end
camera latency or drift. The player fixture also now verifies actual XTest
Play/Pause, clock pause, fullscreen, Escape, natural EOF and replay under owned
Xvfb, with IPC observation only. Source MP4 bytes remain unchanged.

The optional input probe includes static Record control rectangles and state
flags. Its100-record quota now counts changes/clicks rather than periodic live
paints. Generated native input startup is gated behind both validation and
capture-test features plus explicit environment flags. Normal packages exclude
these features. Actual desktop Record, physical latency/drift/unplug, fresh OS,
Google-account and Windows/macOS acceptance remain unverified.

## Native camera motion and backlog repair0.1.4 · 2026-10-20

Physical C922 metadata exposed dynamic exposure reducing selected30fps to15.46.
Temporarily disabling it settled raw frame delivery at29.999fps. Capture now
uses an optional V4L2 control guard: change1to0, restore only when still0after
the child stops, and leave unsupported controls/other changes untouched. Mock
and ABI checks plus a real capture verified the decision and restoration.

The actual1080p recording plus720p RGBA preview pipeline delivered26.13fps with
two x264 threads, then30.05fps using up to four available logical CPUs. CRF20,
veryfast preset, source/preview dimensions and normalized media clock remain.
Core generated take verification passes exact frame counts, copied interior
frames, H264/AAC timing and recovery with the changed thread count.

Overlay rendering now has no pending input snapshot and one replaceable result;
the UI converts/uploads after releasing that mailbox lock. Epoch protection and
stationary-view retry remain. A regression fixture covers replacement and no
queued job. Record also has a stable interaction ID: the existing generated
capture flow failed after an error-widget transition before the fix and passed
in11.34seconds after it.26studio tests and strict app/core lint pass; core unit
tests89passed across the main run plus a targeted rerun of three sandbox-denied
loopback tests. Opt-in hardware/Google/platform scopes are not inferred from them.

The generated-only benchmark script isolates source-clock and filter-clock
ages. Compressed producer/pipe backpressure was not treated as physical camera
latency, and output reorder/removal of frame normalization were not adopted.
Physical camera checks used no microphone, viewed no images, made no takes and
deleted their temporary media. End-to-end sensor-to-screen delay and OBSBOT
visual acceptance remain unverified.


## Native Record studio 0.1.5 — 2026-10-21

Repaired late canceled video/insert/card-image results, imports queued for the
project lock, invisible first-import errors, failed-preview startup cache reuse
and uppercase MP4 playback. Recorded takes and saved source files are retained.

The native workspace passed all-feature tests (32 studio and 90 video-core
unit tests), formatting and strict all-target Clippy. Separate generated checks
passed capture/save/recovery, preservation of a recorded take while the project
lock blocks import processing, preview/export/cache/cancel and newer drafts,
both encoded shapes with exact frames/audio/cards, and insert import/seek/cancel.
Fresh owned Xvfb displays passed actual native Play/Pause/fullscreen/Escape/replay
with an uppercase MP4, plus Record hold/tap and standard Start/Stop at default
and minimum window sizes. The interview-editor/server workspace also passed.

The normal-feature release was packaged as
`storyfold-record_0.1.5~preview.1_amd64.deb`, extracted, hash/dependency/launcher
checked and kept running for eight seconds without startup errors. The matching
managed user app was atomically updated and started; existing windows and
projects were preserved. Isolated installer checks passed.

Physical sensor-to-screen timing, OBSBOT, real file-picker and desktop visual
acceptance, fresh-OS/Windows and real Google/keychain/publishing checks remain
separate. No physical device or paid AI was used and no video was published.
