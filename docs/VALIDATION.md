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
