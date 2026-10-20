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

The studio folder picker passes explicit local paths to video-core. Project and
Git errors keep the in-memory draft. New/Open cannot overwrite concurrent draft
edits while loading; Save can run during edits and acknowledges only its snapshot.
All source writes and commits continue through the core's confinement and secret
checks. The studio exposes no shell text field or automatic push.
