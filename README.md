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
you explicitly enable continuous capture. The system's preferred detected
microphone is selected initially. Check its name on the preview badge before
enabling the camera; **Mic off · silent takes** means new recordings will be
silent. An explicit Microphone off selection stays off through refreshes.
Linux microphone discovery requires `pulseaudio-utils`, included in the Debian
package dependencies. Hold Record/Space for a clip, or tap twice for a
long take. Stop camera before changing projects or shape. **Add media** imports
a local video; **Project → Recover interrupted recordings** preserves existing
clips while adding missing takes. Saved media stays intact during timeline edits.

In **Your sequence**, drag either grip on a clip to trim its source in/out; drag
the middle of a clip to move it to the insertion marker. Click or drag the ruler
to seek, then use **Split at playhead** to cut. **Fit**, zoom and **Focus clip**
make long sequences and short clips reachable. Each released drag is one Undo
step; Escape cancels an unfinished drag. The camera-off stage shows the local
source frame while seeking and trimming, including source revealed by extending
a handle. Full sequence playback and audio use **Preview sequence**.

Clips show source audio waveforms, including after trimming, splitting or moving
them. Analysis runs locally in the background and preserves the original media.
Loading, silent audio, no audio track and decoder errors are distinguished;
right-click an errored clip to retry. A waveform cannot recover sound that was
never recorded. Stop the camera, choose a microphone and make a new take if the
previous one is silent. Source scrubbing shows frames without playing audio.

Edit one of nine cards, add exact text/image and highlights, then **Approve**.
**Show** (keys1–9) adds a five-second cue during a take or at the playhead.
Later slot edits retain existing cue content. Project saves and local versions
use confined atomic writes; nothing auto-pushes.

On Linux, **Preview sequence** plays a snapshot of your current cuts directly from the local
source clips, with audio, cards and inserts applied during playback. It opens the
native mpv player without rendering a full MP4 or saving a project version first.
Keep editing and press Preview again to play the newer edits. Space pauses,
arrows seek, and F or a double click expands it. Escape leaves fullscreen;
Q closes the player. An active recording still needs to finish saving its take
before playback opens.
Other platforms retain the rendered MP4 compatibility preview.

**Export MP4** renders the finished sequence into the project's `exports/`
folder, shows actual encoding progress and permits cancellation before publication.
**Open export folder** reveals the result. Exports save the submitted snapshot
and a local Git version; edits made during rendering remain an unsaved draft.

**Project → Prefer NVIDIA GPU encoding** is enabled initially for each launch.
Recording and sequence rendering test NVENC availability in the background before
starting, then use the NVIDIA encoder when available. Progress names the selected
backend. Turn the preference off while idle to use CPU encoding for subsequent jobs.
If NVENC cannot initialize during that check, the job uses CPU encoding. A later
encoder failure is reported; interrupted recordings retain their recovery files.
Audio, video decoding, scaling and card composition still perform CPU work.
This release does not provide AMD or Intel hardware encoding.

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
