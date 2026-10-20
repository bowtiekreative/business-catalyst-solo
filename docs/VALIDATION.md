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
