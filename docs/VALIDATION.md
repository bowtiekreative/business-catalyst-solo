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
