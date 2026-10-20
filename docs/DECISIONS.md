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

## Sidebar Video · 2026-10-19

- Ryan confirmed the detailed native egui/eframe specification and local AI.
  Begin with milestone 1 only. The working Dioxus Record studio remains available;
  no existing Storyfold launcher, project or runtime is replaced by this scaffold.
- Add `apps/sidebar-video` and `packages/video-core` to the current workspace.
  Keep the existing Catalyst app/core/CLI paths unchanged. The new desktop uses
  exactly the existing `eframe = "=0.33.0"` feature list: AccessKit, default fonts,
  glow, Wayland and X11. Use existing crates and the existing target/toolchain.
- Cargo package/binary is `sidebar-video`; the window is branded Storyfold and
  identifies itself as a foundation. No pretend clips, source picture, recording,
  export or AI output. Only in-memory name and shape controls are functional. User-facing pending-feature
  copy uses plain language; milestone numbers stay in these documents.
- Milestone 1 uses exact landscape 1920×1080 and portrait 1080×1920 dimensions,
  nine stable one-based slots and six overlay kinds. Actual project/timeline,
  save/load, cue, undo and crash-safety invariants remain milestone 2 work.
- Bundle the official, unmodified Inter 4.1 static TTFs at weights 400 and 600 plus
  the upstream SIL OFL. No font fetch runs in the app. Source and SHA-256 hashes
  are in the font directory's README.
- Preserve Storyfold branding with violet `#7860b5` (hover `#66509d`) and ivory
  `#f7f6f2`, superseding the standalone prototype's blue/cool-gray palette. Keep
  the specified radii and dark ink/body tokens. Muted text is placed on white/surface;
  canvas text uses body/ink to preserve 4.5:1 contrast. Pending-feature controls
  remain readable, labelled and disabled in AccessKit. Motion is static, so
  reduced-motion preferences require no separate setting in this milestone.
- No AI runtime is selected or contacted by this app yet. The coordinator's
  read-only discovery reported local Ollama on port 11434 with `qwen3.5:9b`,
  `llava:7b` and other installed models; LM Studio on port 1234 was unavailable.
  These are inventory observations, not evidence of vision/search/overlay
  capability or model quality. This milestone makes zero inference requests.
- Confirmed local AI supersedes the earlier paid Claude/ChatGPT provider choice
  and the paid-request budget STOP. Do not add a cloud key/budget gate or silently
  fall back to a paid provider. Future web discovery is still explicit network
  work, distinct from local inference. Models/search/approval UI come later.
- Reuse public `catalyst_core::check_secrets` now. Path/private helpers are
  crate-private, so persistence must obtain a reviewed shared API later rather
  than duplicate or silently broaden Catalyst's code paths. No new I/O now.
- Chromium screenshot sandbox/output compatibility is not probed in milestone 1;
  there is no screenshot subprocess or web-page fetch in this foundation.
- Packaging, live camera timing/audio drift, renderer parity, local AI quality,
  OAuth client creation, real uploads and Windows interactive accessibility are
  later acceptance work. No milestone beyond the foundation is claimed here.
