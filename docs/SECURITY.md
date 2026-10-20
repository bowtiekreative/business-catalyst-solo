# Security boundaries

The app opens only an explicitly selected local folder. It has no HTTP listener,
AI request, automatic network access or deployment command. Never open a folder
from an untrusted source without reviewing it first.

Source reads reject traversal, dot folders, generated folders, symlinks,
unsupported extensions, oversized text and recognizable credential patterns.
Saving also rejects hard-linked targets, nested repository roots, dirty Git
state, Git attributes, tracked symlinks and submodules. Scans are read-only;
unknown sections remain custom and are never reconstructed from guessed markup.
Dynamic components, relative links and metadata expressed as code need a real
stack adapter and rendered validation.

Every save rechecks the reviewed content, scans tracked files and the replacement
for likely credentials, persists recovery data, replaces one source file, and
makes a scoped local commit. Git hooks, fsmonitor, signing and global attributes
are disabled for app-owned commands. No shell command is accepted from the UI.
The starter requires Git attributes to be absent to avoid executing custom clean
filters. It does not publish or push.

Snapshots live in `.catalyst/private/`, which must be ignored and untracked.
They include before/after source. On Unix their directories are created with
mode 0700 and new files with private permissions. Windows uses the folder's
inherited ACL. Treat the whole local folder as private; no credential scanner
can identify every secret. Do not store credentials in website source.

Atomic replacement preserves existing source permissions. A failed commit
restores the earlier source and clears only the file staged by Catalyst. One
save lock prevents concurrent Catalyst saves. Avoid editing or staging the same
repository from another program during a save; this starter is not a sandbox
against a hostile concurrent local process. Pending recovery snapshots can
remain after a crash. Inspect them and Git status before removing a stale lock.

Changes are held in memory until reviewed. The close prompt protects a normal
window close, not an operating-system crash. History is local, is not synced,
and is not a substitute for repository backups. Undo refuses to overwrite a file
that no longer matches the saved version. A history write failure after a
successful commit reports the commit; inspect Git before retrying.

The bundled HTML prototype is reference material. Its external browser resources,
sample claims and optional JavaScript helpers are not trusted Rust runtime code.

## Sidebar Video milestone 1

The separate `sidebar-video` executable opens no project, camera or microphone,
executes no subprocess, contacts no service (including localhost models), reads
no credentials and has no persistence. Only its in-memory name and orientation
controls are active. Names use Catalyst's public `check_secrets`; rejected names
leave the last valid core value intact. Bundled font data is compiled into the
executable, with no runtime font download.

Future project persistence must reuse Catalyst's confinement/private-folder
patterns and credential scanner rather than copying implementations. Confinement
and private-directory helpers are currently crate-private; their later reuse
needs a reviewed shared API before video writes are introduced. This milestone
does not expose or change those APIs and does not claim filesystem invariants
that have not been implemented.

Later capture, local model calls, web screenshots, exports, project Git and
YouTube need explicit actions, cancellation, constrained paths and dedicated
validation. Local AI is the confirmed direction; no cloud provider/key or paid
budget flow is added here. Local inference can still disclose content to a
separate local process, so it must remain explicit when implemented.

The optional `validation-screenshot` feature enables eframe's existing
`EFRAME_SCREENSHOT_TO` validation output. Only an explicit test launch supplies
that path; it is not a project-save feature and normal builds do not enable it.

## Sidebar Video persisted core · milestone 2

`Studio::from_json` denies unknown fields and validates schema version, bounded
item counts, 8 MiB JSON size, positive typed identities, references, unique media
paths, clip/source bounds, nine ordered slots and cue bounds. Durations have no
arbitrary interview-length cap; checked `u64` millisecond arithmetic rejects
overflow. `Studio::to_json` validates again and runs the shared credential scan.
Use these boundary methods rather than treating unchecked serde output as a
validated project. Media metadata does not prove codec or measured duration;
future capture/import must probe that information.

Project paths must be portable normal relative components under `media/`.
Traversal, dot components, backslashes, Windows device names and nonportable
filename characters are rejected. Existing file/parent checks reject symlinks
and hardlinks. Windows hardlink checks query Win32's open-file information;
that target has not been run locally. Private folders use Unix 0700 and temporary
files use private default permissions. Windows retains inherited ACLs.

The reviewed public helper extension is `catalyst_core::local`. Website reads
retain their old generated/dot-name and extension policy. Video's explicit
project policy allows JSON and new file leaves only under checked parents;
the separate metadata policy permits only the known root `.gitignore`.
Private namespaces are a fixed enum, not arbitrary hidden paths. Atomic writes
require caller confinement and recheck expected bytes just before publication.
The helper's raw path interface must not be exposed as arbitrary UI writes.

Save locks are OS advisory locks in `.sidebar/private/save.lock`, released when
the process exits, including abrupt exit. The inert ignored file is retained;
deleting it to force access can create competing lock ownership and is unsafe.
Same-directory temporary writes are file-synced before rename; Unix directory
metadata is synced after rename. Mirrors prepare first and the complete
`project.json` publishes last. Failure before publication leaves the old project
loadable. A crash can leave an ignored lock file or temporary/derived files;
these are not evidence that the authoritative project was replaced.

Stores compare the last loaded/saved bytes and Git HEAD under the lock. Outside
changes refuse a save without replacing the in-memory draft. These checks protect
cooperating app windows and ordinary stale saves; they are not an OS sandbox
against a hostile local process swapping a path between check and rename. A
post-rename filesystem flush failure may leave a complete new file despite an
error; retain the draft and inspect/reopen rather than assume no file changed.

Only a newly app-created folder is Git-initialized. Existing projects must own a
normal repository with the expected 11 data files tracked, media/private paths
ignored and no Git attributes or pre-staged changes. Git uses argument vectors,
disables hooks/fsmonitor/signing/global attributes, clears inherited repository,
index/object and config-injection redirects, and never accepts UI shell commands.
Only the 11 exact data paths are staged; unknown overlay files stay untouched and
untracked. Before each commit, exact project/overlay data and its summary pass
the shared credential scanner. No remotes or pushes are created.

Timeline deletion never unlinks source files. Save-version commit failure keeps
latest valid saved JSON; its error explicitly distinguishes saved edits from a
failed local version. UI integration must keep newer edits when a worker returns
a save acknowledgment. No capture, inference, network, export or upload client is
introduced by milestone 2.

## Continuous capture and media import · milestone 4

Only explicit `CaptureSession::start` opens camera/audio inputs. Linux enumeration
uses QUERYCAP/ENUM_FRAMEINTERVALS and Pulse inventory without starting a stream.
Start rechecks the selected detected device IDs and queried 30 fps mode. It accepts
no arbitrary input URL, filter graph or command string. A disabled microphone
uses silent local audio so MP4 preparation still has a consistent audio track.

FFmpeg-sidecar is pinned to 2.7.0 with default features disabled; its downloader
is absent. The app resolves installed system tools, passes fixed argument vectors,
and never invokes a shell. Probe/import rejects URLs, device nodes, playlists,
symlinks and hardlinks. Media inputs are protocol-limited to local file/pipe.
Probe/cut subprocess output is bounded, diagnostics are not reflected as raw
paths, and jobs have cancellation and timeouts. The cut processing budget scales
with the selected duration; it is not a fixed interview-length limit.

Each capture owns a new private 0700 session folder on Unix and an OS advisory
session lock. Source segments stay ignored. A malformed/oversized manifest or
unsafe descendant path is rejected before recovery work. Recovery refuses a live
session lock and accesses only manifest-owned deterministic segment filenames.
It does not reinitialize a project or resume a camera. A prematurely stopped
tail is probed for complete frames; approved marks beyond the finalized duration
are skipped and retained mark durations clamp before Editor append.

Capture commands and events have bounded queues. A missed release caused by a
full command queue requests camera stop rather than leaving an invisible take.
Event overload stops capture and preserves the recovery manifest. Camera EOF,
stall or unplug reports an explicit error and keeps completed media and remaining
segments. Drop requests stop, cancels preparation and reaps the camera process;
the UI must keep active/finishing sessions visible until `can_leave()`.

New binary media uses the shared confined `PendingFile` helper: private temporary
file, file fsync, atomic no-clobber publication, and Unix parent-directory fsync.
Any existing destination remains unchanged, including a collision after file
preparation. Import copies in fixed-size chunks, checks cancellation and rejects
an ordinarily changed source. It never follows a growing file indefinitely or
loads an entire video into RAM. No timeline removal or cancelled take deletes
an existing source file. As with project saves, hostile concurrent local path
replacement and hardware power-loss guarantees are outside this workstation's
verified boundary.

Generated integration tests require both an opt-in Cargo feature and an explicit
test environment value. Their sources are fixed lavfi patterns/tone; normal
device inventory never substitutes them for missing hardware. Synthetic timing
and duration checks do not establish physical capture latency, microphone sync,
unplug behavior or ten-minute drift acceptance.

The studio folder picker passes explicit local paths to video-core. Project and
Git errors keep the in-memory draft. New/Open cannot overwrite concurrent draft
edits while loading; Save can run during edits and acknowledges only its snapshot.
All source writes and commits continue through the core's confinement and secret
checks. The studio exposes no shell text field or automatic push.

## Shared overlay rasterization · milestone 5

Rendering is a pure in-memory operation after explicit asset preparation. It
accepts validated approved content and the two supported project output sizes,
not arbitrary filters, shell text, URLs or file paths. The immutable cue snapshot
includes motion/highlight settings; mutable slot edits cannot alter an existing
cue's rendered words. Legacy missing content remains an explicit error.

Image preparation reuses ProjectStore ownership and Catalyst path confinement,
rejecting traversal and linked/nonregular descendants. Only encoded PNG/JPEG/WebP
is accepted, read through a bounded stream and decoded with dimension/allocation
limits. The cache also enforces an aggregate decoded-byte limit. No image parser
runs from a per-frame filesystem read. Preparation failures preserve already
loaded assets. Changing projects requires clearing the cache, preventing a same-
name image in one project from being substituted into another project's overlay.
Explicit reload is required after changing a cached image file.

Only bundled licensed Inter 400/600 faces are loaded; no host-font discovery or
runtime font download is used. Unsupported characters, missing images, overflowing
text and more than nine overlapping overlays produce visible caller errors.
Copied text is never silently shortened to satisfy the geometry. Resource limits
bound decoded images, active layers, cached layouts and raster glyph entries;
these are image/frame limits, not interview-duration limits. Filesystem race and
cross-platform caveats in the project-safety sections continue to apply.

The raw frame contract is straight RGBA with zero color in transparent pixels.
No video encoder, export write, AI provider, network client or upload is introduced
by this renderer. Generated proof PNGs are an explicit unit-test-only option;
they never appear as a real camera, user photo or approved project content.

## Sequence export · milestone 8

Export accepts a Studio snapshot and an optional normal MP4 filename leaf inside
its project's exports/ folder. It accepts no UI shell/filter text, arbitrary
output directory, URL, device or network destination. Explicit system FFmpeg and
ffprobe receive fixed typed argument vectors and file/pipe protocol restrictions.
Only generated safe relative filenames appear in private concat manifests.
Source video and image paths reuse ProjectStore ownership and Catalyst confinement;
links/nonregular files, ordinary source changes and output collisions fail.

The snapshot and approved content pass existing validation/credential scans.
No sequence cue is silently discarded for missing approved content, images,
overflow or unsupported glyphs. All applicable images are checked before encoding;
active-image cache preparation is explicit. Raw frame transport and diagnostics
are bounded and jobs have cancel/stall/scaled processing timeouts. Actual source
and completed video remain outside Git as required by the project ignore rules.

The MP4's temporary path is app-generated. Publication never overwrites an
existing export, even if a destination appears after preparation. Cleanup unlinks
only registered generated private workfiles and attempts to remove an empty work
directory; it never recursively deletes unknown files or project media. Existing
filesystem race/post-rename flush/power-loss caveats still apply.

After a valid MP4 is published, cancellation stops being available during the
small local version step. ProjectStore saves against its original baseline,
scans exact data and uses hardened local Git; it never pushes. Outside changes
refuse the save while retaining the completed MP4 and newer draft. Git failure
also preserves saved JSON and the MP4, and is reported separately from successful
media creation. UI code must retain the submitted snapshot baseline on an ack,
not overwrite newer edits with it. Export introduces no upload, inference or
background network action.

## Visual inserts · milestone 7

Insert JSON accepts a project-relative media path, typed placement/zoom, integer
centers/percent and positive duration. It accepts no remote URL, filter expression,
shell command or arbitrary output path. Existing content credential scanning and
project/snapshot byte bounds remain. Storage now validates every media_paths()
reference, including immutable Removed cues, through existing link/confinement rules.

Video import reuses explicit file import and preserves original media. Proxy creation
uses a unique confined PendingFile, fixed local-file FFmpeg flags, actual progress,
cancel/timeout and source metadata checks before no-clobber publication. Canonical
metadata bounds one square-pixel/muted30fps H.264 track, even axes and1920px maximum
axis. Invalid input is refused or must be explicitly prepared; unsupported audio
mixing is not silently introduced. No camera, inference, network or upload occurs.

Only one visual insert can overlap. Image cache limits are unchanged. Raw decoder
frames are at most16MiB, with one current frame, one queued frame and one producer
buffer per decoder; prepared premultiplied insert memory is also bounded. Diagnostics
are drained with a65KiB ceiling, and stalled requests have a30s timeout. Cancellation,
seek, clear and Drop kill/reap children, close the frame receiver and join readers.
A cancelled or changed source never silently reuses an earlier successful frame.

Asset preparation is explicit and worker-only. Scene rendering and source thumbnails
read prepared memory only. Source fingerprints and project baselines are rechecked
before MP4 publication/save, preserving M8's output/version failure behavior. Existing
filesystem race, post-rename flush and platform caveats remain. Behind is an opaque
camera card, with no claim of person segmentation, matte handling or background removal.

Scene parallel rasterization uses no unsafe aliasing: scoped workers receive
separate scratch Pixmaps and share read-only source bytes. At most four reusable
output-sized rasters add about31.7MiB at full HD, independent of video duration.
Threads join before rows are copied back. Validated snapshot caches hold at most
18 complete definitions and compare every field before reuse; changed secret text
or invalid zoom remains an error. The compositor's integer alpha rounding is
checked against the pinned library, including translucent destination cases.

A generated regression demonstrated that extension checks alone allowed a renamed
ffconcat file to follow a normal-name symlink outside the selected folder. Generic
video probes/normalization now allow only the supported self-contained container
demuxers, while canonical insert/sequence playback explicitly selects MOV/MP4.
Private app-generated concat manifests retain their intentional fixed concat path.
This closes that content-based bypass without relying on a filename extension.
