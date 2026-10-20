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

## Sidebar Video · milestone 1

```sh
cargo run -p sidebar-video
```

This opens a native Storyfold-branded Record studio layout with bundled Inter
400/600 fonts, a real in-memory name and shape switch, nine empty overlay slots,
a take bin, camera stage and empty clip/overlay tracks. Export, recording,
imports, clip tools and the local AI builder are visibly disabled and labelled
as later milestones. It makes no inference, network or device requests, opens no
project folder, and writes no project files. Name and shape changes last only
for the current window.

`packages/video-core` owns output geometry, validated slot identities and studio
settings; `apps/sidebar-video` draws and forwards events. Later milestones add
project/timeline operations, recording, a shared preview/export renderer, local
AI, inserts, MP4 output and YouTube. The current Dioxus Storyfold application
continues separately until this native implementation reaches feature parity.

Inter font provenance and the SIL Open Font License are included under
[`packages/video-core/assets/fonts`](packages/video-core/assets/fonts/README.md).
The existing Ubuntu/Windows workspace CI automatically includes both new members;
platform success must be confirmed by an actual CI run.
