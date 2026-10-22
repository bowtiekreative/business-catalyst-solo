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

The performance checkpoint composites only each card's conservative pixel bounds,
with a one-pixel transparent sampling margin, instead of resampling a transparent
full-HD layer for every card. Full project-size output and all layout/motion math
stay unchanged. Exact content/geometry/image-size cache keys also retain successful
content validation: every changed snapshot revalidates before it can enter the
cache. The test-only old full-layer path proves byte equality and is absent from
normal builds. Camera capture/preview frame rate is unaffected by this optimization.


### Native capture controls and manual card approval

The egui controller explicitly enables one continuous core session after device
selection. Discovery, startup, local import and recovery run on worker channels.
Pointer release outside Record and Space release reach the core gesture controller.
Mixed input does not duplicate a take; focus loss finishes its active range.
New/Open/close and shape changes wait for capture and media work to finish.
Completed takes append through one core transaction to the current Editor, then
save on the existing worker. Recovery skips media already present in the model,
retaining timeline edits and Removed clips. Short append validation uses try-lock;
painting never waits for a long import or save. Session reaping runs off-thread.

Manual cards remain drafts until Approve. Text blocks, Unicode selections,
highlight style, motion and an explicitly imported image form one core snapshot.
Edit::AddCueAtPlayhead owns clip lookup and end clamping; the app has no second
timeline calculation. Cue snapshots remain immutable after later slot edits.
PNG/JPEG/WebP import validates bounded decoding before atomic publication.
The card worker uses one-job/one-result bounded channels. Font shaping, image
preparation and rasterization stay off painting. Assets are reused until the
project/image set changes. Project epochs reject late frames. The app uploads
straight RGBA without another layout path. Reduced motion invalidates preview.

## Sequence MP4 export · milestone 8

`video_core::export::export_sequence` is an explicit blocking worker operation.
It takes an immutable Studio snapshot, the ProjectStore baseline captured at job
start, options, the existing Cancellation token and a progress callback. The UI
forwards those values and preserves its newer-draft/save-ack guard. Export never
reopens a fresh save baseline at job end or replaces an in-memory Editor.

ExportPlan validates a nonempty sequence, approved content and cumulative30fps
frame boundaries. Rounded absolute sequence boundaries avoid accumulated per-clip
rounding; a clip shorter than one output frame is rejected explicitly. Horizontal
1920×1080 and vertical1080×1920 outputs use H.264/yuv420p and AAC48kHz stereo. A
vertical sequence over180s returns a typed warning without imposing a length cap.
Removed cues do not play; a kind-only cue in the active sequence is an error.

Each trimmed source is normalized into exact-frame video and exact-length stereo
PCM in a new app-owned private work directory. Generated relative-name concat
lists join these streams, and a final FFmpeg process combines them with streamed
straight RGBA frames from the same renderer used for preview/timeline playback.
Only one RGBA frame is held by the writer; pipe backpressure bounds buffering.
At cue changes the worker explicitly prepares the active images, then rendering
uses memory only. Long sequences do not need every image decoded simultaneously.
Audio is encoded once into the final AAC track, avoiding accumulated AAC priming
at clip joins. Intermediate WAV uses RF64 automatically when needed, avoiding
the classic RIFF4GiB size limit on long interviews. The baseline normalizes video before final overlay encoding; this
uses temporary disk and a second H.264 encode, a documented initial tradeoff.

FFmpeg's actual `-progress` output drives monotonic phase progress. Protocol lines
and diagnostics are bounded; cancellation/timeouts kill/reap the child and unblock
and join its frame writer. Real output writes keep fast-start muxing alive but
never become invented encoding percentages. A failed frame worker aborts the
child. Temporary cleanup removes only registered generated files, retaining any
unknown/linked files rather than recursively deleting them.

Before publication, the final MP4 is probed for its codec, geometry,30fps, exact
frame count and audio/video duration. Sources are checked again for ordinary
changes. Shared PendingFile performs confined no-clobber/fsync publication inside
exports/. Cancellation before publication leaves source media and existing
exports intact. After publication, Save and Save version use the original store
baseline and existing hardened local Git implementation. A save/version failure
returns the valid MP4 plus explicit project_saved/version_error evidence; it does
not delete the video or silently overwrite another window's project. An unchanged
snapshot can reference its existing local version instead of an empty commit.


### Native export controller and sequence player

The egui Output controller snapshots the Editor and immediately displays progress
before starting a worker. It serializes store operations with New/Open/Save while
allowing timeline edits. Only the submitted snapshot is acknowledged after an
export; a newer draft is never overwritten or marked clean. Reduced-motion changes
also invalidate an automatic preview. Preview reuses only an exact current
snapshot/motion match; Export always builds a new confined artifact. Cancellation
is available until atomic publication. A saved MP4 remains available if local Git
versioning fails, and that separate error is visible.

Sequence playback opens the actual rendered MP4 in a separate native mpv
window, with fixed typed arguments and only a confined project exports/*.mp4 path.
Its visible controller provides Play/Pause and fullscreen. Space pauses, arrows
seek, F/double click toggles fullscreen, Escape leaves fullscreen, and Q closes
the player. This is distinct from the live camera stage; it is not an embedded egui
sequence decoder. The controller starts/reaps players off painting, rejects stale
project results and exposes errors instead of leaving a blank stage. The export
folder opener also runs off-thread, with bounded waiting and visible failures.

## Visual inserts and zoom · milestone 7

`OverlayContent.insert` is a serde-defaulted immutable InsertContent snapshot.
Existing six-kind/photo-card projects load unchanged. `from_insert` creates the
explicit insert-only Photo alternative; ambiguous card text/image/insert mixtures
are refused. The title is metadata. `media_paths()` lets storage/export apply the
same project confinement to legacy images and new insert sources. All cues still
belong to their clip, so move/undo/save retain the approved insert definition.

ZoomPoint stores source-relative integer center and100–250% zoom; ZoomSpec stores
start/end, duration and Smooth/Steady/Snappy easing. The core computes cover crop,
interpolation/clamping and box-to-model conversion from absolute time. Reduced
motion freezes the approved start box. The integer-millisecond clock tolerates
two-thirds of a millisecond when mapping rounded camera/cue differences to the
30fps source grid:133ms−67ms is66ms but still represents two frames. The chosen
source-in frame and elapsed-frame count are quantized separately, so fractional
source offsets cannot cause phase-dependent repeats or skips. This is a clock
quantization tolerance, not nearest-frame rounding:5999ms remains frame179,
and the decoder still refuses indices outside the actual proxy frame count.

SceneLayout owns both shapes and three layouts. Fullscreen hides the camera image;
Beside splits landscape horizontally or portrait vertically. Behind places an
opaque aspect-preserved camera card over full-frame media. It does not isolate a
person or remove the camera background. Video inserts are visual/muted; the
sequence's existing narration audio continues.

Image preparation reuses the existing confined cache. Explicit worker operations
import/prepare a muted H.26430fps square-pixel proxy, retaining the imported original.
The proxy has even dimensions with long axis at most1920px. Canonical frames and
integer-second seek plus an exact remainder trim make seek repeatable instead of
assuming arbitrary VFR timestamps are frame indices. inspect_video validates
metadata for offset/duration controls; no probe happens in painting. The cache
prepares one requested video frame tagged by path and source frame. A missing,
changed, ended or wrong-time asset is an error, never an old frame substitute.

`prepare_live/prepare_timeline` do explicit worker IO; `insert_preview` returns the
prepared natural-aspect source thumbnail. `render_scene_live/render_scene_timeline`
perform no file reads/decodes and compose straight RGBA using the same card renderer.
Overlay-only APIs explicitly reject inserts. One visual insert may be active;
overlap errors are editable and cards can still appear above it. Camera input is
unpadded natural-aspect RGBA, including4:3 modes; capture supplies mode-derived
preview geometry. Empty camera UI references remain a caller decision, not media.

M8's external API remains unchanged. Sequences containing inserts normalize camera
clips with known integer aspect-preserving padding, decode through a bounded raw
frame reader and strip only those known margins before shared scene composition.
Final encoding receives opaque scene RGBA plus the existing exact PCM narration
sequence. No-insert sequences retain their existing overlay-stream path. Each
reader has a capacity-one channel, current frame and one producer work buffer;
preparation additionally owns one premultiplied insert frame. Decoder/asset memory
is bounded by frame/image limits rather than sequence duration. Cancellation closes
receivers before joining blocked producers and kills/reaps decoder children.

Large scene panels use at most four reusable full-HD scratch rasters, each drawing
only its assigned rows with the original global tiny-skia Pattern coordinates.
This preserves sampling/rounding while parallelizing expensive image fills. Identity
card composition uses the same /255 byte rounding as the pinned tiny-skia path,
skipping transparent pixels; exhaustive and full-frame reference proofs gate it.
Exact immutable insert/asset snapshots reuse bounded validation entries, while a
changed snapshot is validated again. No font/image quality or camera FPS is reduced.
The measured final scene still exceeds the30fps frame budget; see VALIDATION.

## Local evidence review · milestone6

`video_core::ai` exposes explicit blocking worker operations for real public RSS
search, isolated Chromium screenshots, dropped PNG/JPEG/WebP evidence, Tesseract
OCR, local Ollama rebuild, proposal validation, usage and approved-card HTML.
Constructing the app/model does not browse, open a device, download a model or
start inference. Search uses Bing's public RSS response, with1–12requested results
(default4); this unversioned adapter is an explicit `shortcut:` and failures retain
manual URL/screenshot options. The model never fabricates search result cards.

Evidence uses bounded decoded images in a new ignored private temporary folder.
Chrome gets a fresh profile without existing login/cookies, sandbox enabled and a
public-only HTTPS CONNECT proxy. The proxy validates/pins each resolved endpoint,
refuses private targets/other ports and bounds connection count, transfer and
lifetime. Capture has a30second timeout and kills/reaps only its owned process
group. Font/image preparation, browser/OCR/inference and file work stay off paint.

The model client uses only127.0.0.1:11434, with proxy/redirect disabled. It checks
that the named installed model reports local GGUF vision metadata and rejects
remote/cloud aliases before sending the image. Strict serde proposals contain only
summary/kind/title/source_url/blocks/highlights. Dynamic JSON schema constrains
visible strings to exact OCR passages and fixes source_url; independent validation
still checks every copied substring, Unicode highlight bounds, content limits and
credential scan. OCR can misread pixels: the review screen explicitly asks the
user to compare source and rebuilt card. JSON remains editable; nothing enters an
approved slot before an explicit approval. Later cue snapshots stay immutable.

Original screenshots copy into owned media only after explicit approval and a
reviewed-byte check. Rebuilt text can approve directly or open the manual card
editor. Standalone HTML exports only an approved slot, escapes accessible text,
embeds the shared renderer's PNG without scripts/external assets, atomically
publishes into exports and attempts a local project version. A failed save/version
keeps the HTML with a separate visible error. Worker acknowledgement never
replaces newer Editor state. Token totals are project-private, model-specific and
summarized for the UTC month; there is no paid fallback or dollar budget gate.
