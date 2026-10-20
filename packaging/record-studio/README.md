Storyfold Record Studio is a native Rust desktop application. It starts with a
new untitled draft; use Project to create or open an explicit project folder.
Camera and microphone remain off until you enable them in Camera settings.

Hold Record/Space for a clip; tap twice for a long take. Stop the camera before
Preview sequence or Export MP4. MP4s and standalone approved-card HTML go into
the project's exports folder. Save version/export creates only a local Git
checkpoint. Source media remains in media even when a take is removed.

FFmpeg/ffprobe/ffplay and Tesseract OCR are system dependencies. Screenshot capture
also needs installed Chrome/Chromium. Local AI requires Ollama on127.0.0.1:11434
with an installed vision model; it never downloads a model or uses a paid fallback.
The default model is qwen3.5:9b and can be edited in the review screen.

The installed Storyfold interview editor and saved projects remain separate while
this Record studio is under acceptance testing. Existing project/media directories
are never removed by package installation. See the repository's docs/VALIDATION.md
for measured results and physical-device/platform checks still outstanding.
