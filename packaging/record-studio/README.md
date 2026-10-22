Storyfold Record Studio is a native Rust desktop application. It starts with a
new untitled draft; use Project to create or open an explicit project folder.
Camera and microphone remain off until you enable them in Camera settings.
The system's preferred detected microphone is selected initially. Check the
microphone badge before enabling capture; **Mic off · silent takes** means new
recordings will have no microphone sound. An explicit Off choice stays off.

Hold Record/Space for a clip; tap twice for a long take. Preview sequence or
Export MP4 stops the camera and saves any finishing takes first. On Linux,
Preview plays a snapshot of the current cuts, audio, cards and media inserts
directly, without rendering an MP4 or saving a project version. Press Preview
again after editing. The native player has Play/Pause, seek and fullscreen
controls; it stays open at the end and Play restarts it. Export MP4 renders the
finished sequence with encoding progress. MP4s and approved-card HTML go into
the project's exports folder. Save version/export creates only a local Git
checkpoint. Source media remains in media even when a take is removed.

Clips display audio waveforms for cutting, with separate loading, silent and
no-audio states. Analysis is local and runs in the background. Trims, splits and
reordering retain alignment to source audio. Right-click an errored clip to retry.
Source scrubbing is silent; use Preview sequence for playback with audio.
Sound that was never recorded cannot be recovered from a silent take.

FFmpeg/ffprobe, mpv, pulseaudio-utils and Tesseract OCR are system dependencies.
Linux microphone discovery needs a running PulseAudio or PipeWire Pulse server.
Screenshot capture
also needs installed Chrome/Chromium. Local AI requires Ollama on127.0.0.1:11434
with an installed vision model; it never downloads a model or uses a paid fallback.
The default model is qwen3.5:9b and can be edited in the review screen.

Project → YouTube opens connection, upload and clip-import controls. Publishing
requires your own Google Desktop OAuth client and browser consent. Credentials
use the OS keychain. Upload defaults to Unlisted and requires explicit review.
Linux packages include a pinned, checksum-verified yt-dlp executable with its
upstream license notices. The older system downloader is left unchanged.
Building a package requires the verified release artifact in the ignored
data/record-studio-tools directory; packaging never downloads it automatically.

The installed Storyfold interview editor and saved projects remain separate while
this Record studio is under acceptance testing. Existing project/media directories
are never removed by package installation. See the repository's docs/VALIDATION.md
for measured results and physical-device/platform checks still outstanding.

Pinned tool source: https://github.com/yt-dlp/yt-dlp/releases/tag/2026.08.19
Artifact: yt-dlp; SHA256:
1fa6733c37ea6fb51c99ad8fe785e7b7e5f3246c9b980230329d4fb72ed8d4d6
Keep the pinned upstream LICENSE and THIRD_PARTY_LICENSES as yt-dlp.LICENSE
and yt-dlp.THIRD_PARTY_LICENSES beside it. The zip executable includes upstream
code under the Unlicense and vendored ISC/MIT code; retain the full notices.
