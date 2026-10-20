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
