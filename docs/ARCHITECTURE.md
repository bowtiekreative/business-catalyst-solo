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
- `apps/sidebar-video/assets/fonts`: unmodified Inter 4.1 Regular/SemiBold TTFs,
  upstream SIL OFL text and source/hash provenance, embedded without font requests.

The new app has no backend, device handles, file store, Git repository,
subprocesses, model connection or network client. No fake takes, camera picture,
overlay generation or export is presented as operational. Settings are ephemeral.

Later milestones put timeline, capture, the shared RGBA overlay renderer, export
and local AI clients in `video-core`, with worker-thread channels to the UI when
needed. They must add their own invariant and integration evidence. The existing
Storyfold Dioxus application remains the working product until feature parity;
this foundation is a separate executable and is not installed over it.
