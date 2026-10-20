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

## Sidebar Video core · milestone 2

- Add the serializable document, timeline operations, 100-snapshot in-memory
  undo/redo, local project store and local Git versions. Keep device/renderer/AI
  work out of this milestone. Native UI integration is a separately verified
  coordinator responsibility.
- Retain the milestone 1 settings/slot API. New UI edits use `Editor::apply` or
  `apply_batch`; a batch publishes one validated candidate and one undo step.
  Separate monotonic IDs per type permit constructing dependent media/clip/cue
  additions as a single batch. High-water counters survive undo, so undoing an
  addition can still leave a metadata change requiring Save.
- Include clip names. Cue offsets are milliseconds into the clip; trims use
  source timestamps. Split-boundary cues belong to the right half at zero.
  Clamp both cue offset and duration after split/trim. Cue type is captured when
  fired so later slot-kind edits do not silently change it. An immutable rendered
  definition/content reference is explicitly deferred to milestone 5.
- Frame nudges round the absolute frame index at 30 fps, producing 33/34 ms
  increments without cumulative drift. Seconds remain exactly 1000 ms. Reject
  edits that remove the entire clip or exceed its media source.
- Do not impose a 24-hour source or seven-day sequence cap. Those limits have
  no tool requirement. Use checked `u64` arithmetic plus document/item resource
  bounds (8 MiB JSON, 10000 media/clips, 100000 cues).
- `project.json` contains the whole authoritative document; nine slot JSON files
  are derived Git mirrors. This avoids a partially saved multi-file document.
  Publish mirrors first, project last; load the project alone. Future immutable
  overlay definitions require an equally explicit publication/recovery contract.
- Extract and generalize the existing Catalyst local primitives, rather than
  duplicate them. Explicit policies preserve website exclusions, add checked new
  JSON leaves and a narrow `.gitignore` exception, and choose the fixed private
  namespace. Reuse the same atomic writer and hardened Git command builder.
  Add expected-byte verification, Unix parent-directory fsync and OS advisory
  locks using the already cached `fs2` crate. All eight existing Catalyst workflow
  tests remain required regression coverage.
- Stable Rust's Windows metadata link-count API remains
  [experimental](https://doc.rust-lang.org/std/os/windows/fs/trait.MetadataExt.html#tymethod.number_of_links).
  Reuse the already cached WinAPI dependency for the small open-file link-count
  query, with explicit safety comments. Windows execution remains unverified.
- Project creation uses a new folder only; `create_in` derives a unique child of
  a selected parent. Git identity is app-owned local metadata (`Storyfold`,
  `local@storyfold.invalid`), not a change to the user's global configuration.
  Disable init templates. Save version commits exact known data paths and never
  stages an unknown file simply because it sits under `overlays/`.
- OS locks release on crash and leave an inert ignored file. Do not offer stale
  lock deletion. Outside data/HEAD changes require reopen/reconciliation, not a
  force overwrite. A Git failure after Save preserves JSON and reports the failed
  version separately. Save acknowledgments never replace an Editor.
- Media import/copy is not part of this core milestone. Existing source paths
  must resolve to regular owned files. The next import/capture API must make file
  creation atomic and explicit; no fake sources are introduced in the normal UI.

## Continuous capture and explicit import · milestone 4

- Follow the continuous-session specification: one system FFmpeg process keeps
  the selected devices open until explicit Stop camera. Record gestures mark
  buffered frame ranges; they do not restart hardware. Device enumeration only
  queries capabilities/modes. Prefer a real 1920×1080 MJPEG30 mode, with honest
  lower-resolution queried options rather than an unverified default.
- Use the verified upstream
  [FFmpeg-sidecar 2.7 command API](https://docs.rs/ffmpeg-sidecar/2.7.0/ffmpeg_sidecar/command/struct.FfmpegCommand.html)
  and disable all default download features. Use system FFmpeg/ffprobe; no tool
  downloader or network adapter is introduced. Read raw preview directly into
  one latest-frame slot rather than queuing frame events for the UI.
- Keep 720p30 RGBA preview clarity and smoothness. Both preview and recording
  split from one normalized 30 fps filter; frame IDs, rather than UI time or
  process-start guesses, identify source frames. Mark the latest received frame
  at the API call. Camera transport/decoder/preview latency is explicitly still
  a physical acceptance measurement. Do not claim the generated test proves a
  first physical frame within one frame of a press.
- Use the documented
  [FFmpeg segment muxer](https://ffmpeg.org/ffmpeg-formats.html#segment)
  with one-second closed GOPs, MPEG-TS segments and a CSV index. Read that index
  incrementally and derive range boundaries from the shared 30 fps frame clock.
  Preserve an unclosed tail for explicit probe/recovery after interruption.
- Re-encode only first/last video segments and copy fully interior H.264. The
  coordinator accepted one AAC encode of the whole selected audio range at final
  mux: repeatedly encoding AAC edges would accumulate encoder priming and complicate
  precise audio boundaries. Generated tests compare decoded interior video bytes
  and selected frame counts; real microphone drift still needs measurement.
- Capture overlay marks own approved `OverlayContent` snapshots and requested
  duration at fire time. Changing/undoing a sidebar slot cannot change recorded
  content. The completed take batches source, clip and snapshot cues in one undo
  step. Skip marks past recovered ends and clamp durations to finalized media.
- Use a bounded controller queue, latest-frame slot and event queue, with a
  separate cut worker. Stop/Drop must release hardware even if preparation fails.
  Preserve failed/pending ranges and published takes for recovery. Never silently
  append recovered content or overwrite another in-memory project.
- Reuse/extend the shared local primitives with `PendingFile` and named advisory
  locks. Streaming private files avoid full-video allocations. No-clobber
  publication keeps any existing destination intact. Source copy checks metadata
  before/after and reads only its original length.
- Import keeps an explicitly selected supported local video container unchanged
  under a generated project-owned name, instead of unnecessarily re-encoding an
  imported interview. Probe validates video geometry/duration; later rendering
  and export use installed FFmpeg decoders. No URL/playlist import is inferred.
- Prune only a known closed session prefix older than 600 seconds and before
  active/pending ranges. Conservative extra retention is preferable to deleting
  a range required for a failed take. Stopped recovery sessions remain on disk;
  no background recursive cleanup or source-media garbage collection is added.
- Non-Linux builds compile an unsupported capture stub. Physical first-frame
  timing, twenty device holds, microphone drift over ten minutes and a real
  unplug remain unverified. The generated source is a separate feature+environment
  gated testing API, never a normal-app camera placeholder.

## Sidebar Video timeline UI

- File dialogs, safe project writes and local Git versions run on a named worker
  thread. A save acknowledgment updates the submitted baseline; it never replaces
  edits made while saving. New/Open block edits while loading and guard unsaved
  drafts. Failed or cancelled operations keep the current draft.
- Bin and sequence draw the same core clip list. Drag and Move left/right forward
  core edits; split resolves global playhead time in the core. Trim nudges are
  immediate core actions, with typed in/out fields applied explicitly.
- Use the existing cached rfd0.17 native folder picker with the XDG portal backend.
  Creating/saving a draft selects a parent and the core reserves a unique new
  project child; no existing folder is reinitialized.
