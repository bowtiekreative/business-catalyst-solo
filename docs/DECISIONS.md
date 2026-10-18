# Decisions

- Ryan requested a runnable starter plus original references, then requested Rust.
  Use Rust for both UI and core. This supersedes the brief's suggested React and
  Tauri/Electron stack. egui/eframe provides the native shell and AccessKit support.
- Keep a Cargo workspace: `apps/desktop`, `packages/core`, `packages/cli`, plus the
  original schema and handoff. There is no browser backend or Node dependency.
- Open folders by explicit path first. Native folder dialogs and cloning follow
  later. Do not silently initialize or reconfigure an existing repository.
- Scan literal source with bounded pattern matching. Keep custom blocks intact.
  Report dynamic-framework limits. Never claim a source scan proves accessibility.
- Start with reviewed whole-file edits. Preserve the Site Map schema exactly.
  Do not expose structured operations until a stack adapter can update the real
  source and propagate routes, links, navigation and sitemap together.
- Require a clean repository for writes. This is intentionally more restrictive
  than merging concurrent external edits. Existing staged changes stay untouched.
- Keep commits local. Auto-push, AI calls and publishing need their own adapters,
  review surfaces and acceptance tests; no prototype simulation represents success.
- Ship original references unchanged, including the catalog's placeholder copy.
  No production license or factual endorsement is inferred from those materials.
- Pin the desktop framework API to eframe 0.33.0. Check dependency upgrades with
  the Rust workflow tests and a native desktop accessibility pass.
