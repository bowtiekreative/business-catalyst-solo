# Catalyst Solo starter

This is a runnable foundation for the supplied Catalyst Solo brief, independent
of Storyfold. It manages one explicitly configured website folder. React and
TypeScript provide the control panel; a localhost Node service owns filesystem
access, scanning and Git operations. The original handoff remains a reference,
with its simulated actions clearly documented.

## Boundaries

- `apps/web`: accessible control panel, page and source views, reviewed edits.
- `apps/server`: loopback API, session checks and the configured website root.
- `packages/core`: scanner, path confinement, change review and Git operations.
- `packages/schema`: the original Site Map JSON Schema.
- `packages/cli`: read-only site inspection from a terminal.
- `fixtures`: disposable static HTML, Astro, Next.js and WordPress examples.
- `handoff`: original brief, clickable prototype, LAKA catalog and build skill.

Source edits are drafts until reviewed. Applying rechecks the original content,
scans for likely credentials and creates a scoped Git commit. Undo is another
reviewed edit. The service never accepts arbitrary shell commands. Unknown
sections remain custom source; the scanner never rewrites them.

The starter implements a browser shell before native desktop packaging. Native
folder selection, structured page/section operations, provider helpers, automatic
push, host publishing, rollback and ongoing site care remain roadmap items.
No production milestone or remote CI result is implied by this scaffold.
