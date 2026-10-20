# Catalyst Solo · Rust starter

A native Rust control panel for one local website. Both the desktop UI and the
core are Rust. The original Catalyst brief and HTML prototype ship in `handoff/`.
The workspace also contains a separate Storyfold-branded Sidebar Video foundation.
It does not replace the working Storyfold desktop app.

## Run

Install [Rust](https://rust-lang.org/tools/install/) (1.88 or newer) and Git.
Windows needs the Visual Studio C++ build tools. On Ubuntu, install the desktop
build libraries:

```sh
sudo apt-get install build-essential pkg-config libx11-dev libxkbcommon-dev libwayland-dev libegl1-mesa-dev
```

From this repository:

```sh
cargo run -p catalyst-cli -- demo ../catalyst-demo
cargo run -p catalyst-desktop -- ../catalyst-demo
```

Or run `cargo run -p catalyst-desktop` and enter a website folder in Overview.
The sample contains two pages missing meta descriptions and a broken Blog link.
Its content is placeholder. The demo command refuses to overwrite an existing
folder and sets Git identity only in the new sample repository.

Open **Pages → Open source**, edit text, choose **Review changes**, and save one
local Git commit. **History → Review undo** restores the earlier text through a
new review. Closing with an unsaved draft asks before discarding it.

Before editing an existing site, commit its current work and add this line to its
committed `.gitignore`:

```gitignore
.catalyst/private/
```

Read-only terminal inspection:

```sh
cargo run -p catalyst-cli -- status ../catalyst-demo
cargo run -p catalyst-cli -- history ../catalyst-demo
```

## Included

- Native egui/eframe window: Overview, Pages, Source editor, History, Activity,
  and the original LAKA catalog (118 entries across 12 families).
- Source scanner for static HTML, literal Astro pages and conventional Next.js
  routes. WordPress theme detection; PHP editing needs a later adapter.
- Reviewed source changes, stale-draft checks, local Git commits, private
  before/after snapshots, undo, path confinement and credential pattern checks.
- Original Site Map schema, four scanner fixtures and Rust workflow tests.
- Original production brief, clickable prototype, renderer, catalog and build
  skill under `handoff/`. Prototype network helpers and publishing actions are
  reference behavior; the Rust app never executes them.

## Check

```sh
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace
```

Run the checks on Ubuntu and Windows before release. The headless UI test is
written to draw all views; visual and screen-reader testing require a desktop
session. See [validation status](docs/VALIDATION.md) for checks actually performed.

## Scope

This is a runnable boilerplate, not the completed production brief. Structured
page/section adapters, visual page rendering, AI providers, task orchestration,
automatic push, host publishing, live rollback, installer signing and MCP are
tracked in [ROADMAP.md](ROADMAP.md). No API key is needed for the starter.

See [architecture](docs/ARCHITECTURE.md), [security boundaries](docs/SECURITY.md)
and [implementation decisions](docs/DECISIONS.md).

## Storyfold Record studio · in development

```sh
cargo run -p sidebar-video
```

Create/open a project using **Project**. **Camera…** discovers devices and lets
you explicitly enable continuous capture. Select a microphone to include it;
Microphone off is the default. Hold Record/Space for a clip, or tap twice for a
long take. Stop camera before changing projects or shape. **Add media** imports
a local video; **Project → Recover interrupted recordings** preserves existing
clips while adding missing takes. Saved media stays intact during timeline edits.

Edit one of nine cards, add exact text/image and highlights, then **Approve**.
**Show** (keys1–9) adds a five-second cue during a take or at the playhead.
Later slot edits retain existing cue content. Project saves and local versions
use confined atomic writes; nothing auto-pushes.

**Preview sequence** renders the current edits and opens the resulting MP4 in
a native player. Space pauses, arrows seek, F or a double click expands it, and
Escape closes the player. **Export MP4** writes into the project's `exports/`
folder, shows actual encoding progress and permits cancellation before publication.
**Open export folder** reveals the result. Exports save the submitted snapshot
and a local Git version; edits made during rendering remain an unsaved draft.

**Build with local AI** searches public pages or accepts a dropped/chosen
screenshot. Review the source beside the rebuilt card, then approve, edit or
reject. The model name is editable; generation uses installed local Ollama vision
models. Approved slots can export standalone HTML. Photo/video inserts provide
full-screen/beside/behind-camera-card placement and draggable start/end zoom boxes,
100–250% zoom, duration and easing. Video-insert audio is muted.

Publishing and installer acceptance are still being connected.
The working Dioxus Storyfold application continues until
this studio reaches feature parity. See docs/VALIDATION.md for checks actually run;
physical-camera timing/drift and Windows acceptance are not established.

Inter font provenance and the SIL Open Font License are included under
[`packages/video-core/assets/fonts`](packages/video-core/assets/fonts/README.md).
The existing Ubuntu/Windows workspace CI automatically includes both new members;
platform success must be confirmed by an actual CI run.
