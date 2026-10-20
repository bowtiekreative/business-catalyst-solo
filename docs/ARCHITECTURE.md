# Catalyst Solo starter

This is a runnable foundation for the supplied Catalyst Solo brief, independent
of Storyfold. It manages one explicitly configured website folder. Rust provides
both the native desktop control panel (egui/eframe) and the core. There is no Node
service, JavaScript frontend or listening network port. The original HTML handoff
remains reference material with simulated actions.

## Boundaries

- `apps/desktop`: native Rust control panel, page and source views, reviewed edits.
- `packages/core`: scanner, path confinement, change review and Git operations.
- `packages/schema`: the original Site Map JSON Schema.
- `packages/cli`: read-only site inspection from a terminal.
- `fixtures`: disposable static HTML, Astro, Next.js and WordPress examples.
- `handoff`: original brief, clickable prototype, LAKA catalog and build skill.

Source edits are drafts until reviewed. Applying rechecks the original content,
scans for likely credentials and creates a scoped Git commit. Undo is another
reviewed edit. The core never accepts arbitrary shell commands. Unknown
sections remain custom source; the scanner never rewrites them.

The starter opens an explicitly entered folder in a native desktop window. Native
folder selection, structured page/section operations, provider helpers, automatic
push, host publishing, rollback and ongoing site care remain roadmap items.
No production milestone or remote CI result is implied by this scaffold.

## Sidebar Video foundation

Milestone 1 adds two independent members without changing Catalyst's scanner,
review, Git or existing desktop behavior:

- `apps/sidebar-video`: native Storyfold-branded egui/eframe Record studio.
  It draws the header, working shape/name settings, take bin, empty stage,
  disabled 88 px Record button, nine empty overlay slots and sequence/cue tracks.
- `packages/video-core`: in-memory studio settings, exact 1920×1080 / 1080×1920
  geometry, bounded slot identities and overlay-kind vocabulary. The app forwards
  settings changes here; the core reuses `catalyst_core::check_secrets` for names.
- `packages/video-core/assets/fonts`: unmodified Inter 4.1 Regular/SemiBold TTFs,
  upstream SIL OFL text and source/hash provenance, embedded without font requests.

The new app has no backend, device handles, file store, Git repository,
subprocesses, model connection or network client. No fake takes, camera picture,
overlay generation or export is presented as operational. Settings are ephemeral.

Later milestones put timeline, capture, the shared RGBA overlay renderer, export
and local AI clients in `video-core`, with worker-thread channels to the UI when
needed. They must add their own invariant and integration evidence. The existing
Storyfold Dioxus application remains the working product until feature parity;
this foundation is a separate executable and is not installed over it.

## Sidebar Video project core · milestone 2

`video-core` now owns a versioned serde project document, stable media/clip/cue
identities, one ordered active clip list, a Removed list, and clip-relative cue
timing. `Editor` applies typed operations to a validated candidate, publishes a
successful batch as one edit, and retains at most 100 prior snapshots plus redo
in memory. Failed/no-op edits add no undo step. IDs keep their high-water marks
through undo, so a new branch does not reuse the identity of an undone clip.

Moving clips carries their cues. Splitting remaps boundary/later cues into the
new right clip; crossing cue durations clamp to the left end. Trimming preserves
cue source positions before clamping offset and duration into the new clip.
Removing a clip retains its source metadata and cues; restoring selects its new
sequence position. Sequence-time lookup and global-playhead split are core
operations. Frame nudges use the absolute 30 fps grid; second nudges are exact.

`ProjectStore` creates only new folders, or opens an existing owned project
without initializing or reconfiguring its repository. Projects contain
`project.json`, nine `overlays/slot-N.json` mirrors, `media/`, `buffer/`, `exports/`
and `.sidebar/private/`. `project.json` is the authoritative complete snapshot;
overlay mirrors are derived tracked data. Loading never mixes newly prepared
mirrors with an older authoritative project after an interrupted save.

Save uses an OS advisory lock, expected-byte comparisons and atomic publication.
The store acknowledges only the submitted snapshot; it does not replace an
Editor or newer in-memory draft. `is_dirty` compares against that acknowledged
snapshot. Save version first saves valid JSON, then creates a scoped local Git
commit. Commit failure preserves the latest saved project and reports that
version creation failed. Ordinary Save does not commit or push.

Shared `catalyst_core::local` extracts confinement, private-folder creation,
atomic replacement and hardened Git primitives. Existing Catalyst wrappers use
the shared implementations with their existing website policy. Video uses an
explicit project policy, fixed Sidebar private namespace and existing owned
media references. Import/capture, immutable rendered overlay definitions,
renderer, inference, export, upload and packaging remain separate milestones.

## Sidebar Video capture and import · milestone 4

`video-core::capture` owns an explicit continuous Linux session. One system
FFmpeg process opens the selected V4L2 camera and optional Pulse microphone,
encodes one-second H.264/AAC MPEG-TS segments and writes 1280×720 RGBA preview
frames. The preview reader drains continuously and replaces one shared latest
frame; UI paint/poll frequency cannot build an unbounded frame queue. Capture
never starts from drawing the UI or enumerating devices.

The same normalized 30 fps video is split into recorded and preview branches.
Frame `n` identifies the same video frame in both branches. Press/release sample
the core's latest complete preview frame at the API call; range starts are
inclusive and ends exclusive. Host `Instant` determines the 350 ms tap/hold
gesture independently of worker scheduling. A stale preview refuses a new take.
This measures a pipeline frame clock, not physical camera-to-screen latency.

The capture controller and take-preparation worker are separate. Camera start,
stop and gesture handling remain responsive while a previous take is prepared.
Only boundary video segments are re-encoded; fully interior video is copied.
Audio is encoded once for the selected range at final MP4 mux to avoid AAC
priming accumulation at one-second joins. Completed media is probed and atomically
published before a completion event. A take's approved overlay content snapshots
are retained with frame-derived offsets, then appended with media/clip/cue edits
as one Editor transaction; durations clamp to the actual finalized video.

An ignored session manifest stores marks, ranges, prepared files and closed
segment indices. Its CSV index is read incrementally. Only known closed session
segments older than 600 seconds are pruned, conservatively stopping at the first
active/pending/protected range. Published media never depends on those segments.
Stopped sessions and failed ranges remain available for explicit recovery;
unknown buffer files are never recursively removed. Recovery takes a session OS
lock, never starts hardware, and can salvage complete frames from a final
interrupted MPEG-TS segment.

`video-core::media` provides explicit bounded local probing and streaming import.
It copies an explicitly chosen regular local video into a private pending file,
checks source stability, probes the copy, and publishes a new owned media path.
Cancellation drops the pending file and preserves the source. The shared
`catalyst_core::local::PendingFile` centralizes streaming fsync/noclobber publication;
`lock_named` generalizes the existing advisory lock for a fixed capture identity.
No whole video is loaded into application memory for import or publication.

The non-Linux capture implementation is an honest unsupported stub. Media import
continues to use explicitly installed local tools. Generated capture exists only
with the `capture-test` feature plus `STORYFOLD_CAPTURE_TEST=1`, through a separate
explicit API; it is never presented as an ordinary detected camera.

The native studio UI (`apps/sidebar-video/src/workspace.rs`) owns only its Editor,
view state and an acknowledged save snapshot. ProjectStore is used on worker
threads for New/Open/Save/Save version. The UI forwards typed domain edits; it
contains no second timeline model or external command interface.

## Shared overlay renderer · milestone 5

`video_core::renderer::OverlayRenderer` is the only overlay layout/raster path.
`render_live` accepts immutable approved content snapshots with elapsed/duration
milliseconds. `render_timeline` selects active clip-relative cues at global time,
uses the core sequence-time lookup and calls that same live renderer. Removed
clips do not play. Intervals are half-open; inactive cues produce transparent
pixels. An active legacy kind-only cue reports missing approved content rather
than inventing text. Up to nine active overlays compose in caller/cue order.

The renderer shapes Inter text with cosmic-text and draws cards, images, glyphs
and emphasis with tiny-skia. Both horizontal and vertical output sizes use core
layouts for lower thirds, statistics, screenshot cards, numbered lists, photos
and quotes. It retains every approved text block, wraps it, measures the complete
card and refuses overflow at readable font sizes. A missing unsupported glyph
also reports an error. No ellipsis or silent copied-text truncation is used.

The two unchanged Inter 4.1 font files, OFL and provenance now live once under
`packages/video-core/assets/fonts`. The app reads public embedded font constants.
The renderer constructs an empty font database, loads only those two faces and
uses a fixed locale; it does not scan installed fonts. Layout and glyph caches
are bounded. `RgbaFrame.pixels` explicitly converts tiny-skia's premultiplied
pixels to straight RGBA for both egui and future FFmpeg `rgba` input. Transparent
pixels contain zero color. Export encoding is not implemented by this module.

`OverlayAssets` explicitly prepares confined project PNG/JPEG/WebP images before
rendering. The decoded cache is bound to one project, limited to 16 images/64 MiB,
with encoded images at most 16 MiB and dimensions at most 4096 per axis. Images
are contained in their card, preserving their complete evidence. Rendering only
uses cached assets and performs no filesystem reads; callers clear/reload after
changing project/image content. Preparation and first rendering belong on worker
threads. Public renderer APIs contain no hardware, inference or network action.

Approved snapshots carry serde-defaulted `OverlayMotion` and `HighlightStyle`.
Entrance is 600 ms with 90 ms text-block staggering, the specified cubic easing
and a 24px fade-up. Marker sweep, underline draw, bounded focus-line zoom and
fade-rest use Unicode scalar highlight ranges mapped through shaped/wrapped
lines. Reduced motion or Still presents the complete static state immediately,
without movement, reveal or zoom. Subsequent AI review and insert/zoom editing
can extend these content/renderer boundaries without a second UI renderer.
