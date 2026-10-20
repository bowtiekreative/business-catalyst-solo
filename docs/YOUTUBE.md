# Native YouTube core

The native studio has worker-side connection, resumable upload and Linux clip
import operations in `video_core::youtube`, with an integrated native YouTube
panel. This checkpoint has no real Google account or publishing acceptance.

## User setup and actions

The user must create a **Desktop app** OAuth client in Google Cloud, enable the
YouTube Data API v3 and configure consent/test users as applicable. Keep the
original downloaded client JSON outside a project and select it through the
app's file picker. It does not belong in chat, Git, build logs or a task record.

`YoutubeClient::system()` constructs the client without credential access,
folder creation, browser activity or HTTP. Explicit worker operations provide:

| Operation | Effect |
| --- | --- |
| `status` | Reads local keychain state under an empty per-user process lock. |
| `configure_client` | Validates selected Desktop JSON and stores it in the OS keychain. |
| `begin_connect` | Creates random PKCE/state and a localhost listener; returns a redacted authorization handle. |
| `finish_connect` | Waits for the matching callback and exchanges its code at Google's fixed token endpoint. |
| `publish` | Starts one reviewed rendered-MP4 upload after an explicit button action. |
| `resume` | Queries the existing session and sends only its remaining source bytes. |
| `disconnect` / `forget_client` | Removes local permission/session state; never deletes a remote video. |
| `forget_pending_upload` | Forgets local resumption state; never silently starts or deletes a remote upload. |

The UI opens `Authorization::browser_url()` in the **system browser** only after
Connect. It must not log this state-bearing URL. Only `youtube.upload` is
requested. That scope does not establish a verified channel name; the connection
means the channel selected during Google's consent. Reconnect to change it.
`ConnectionId` from the displayed review must be forwarded unchanged at Publish
or Resume; a changed client/account cannot inherit a stale review.

Privacy defaults to **Unlisted**. `UploadedVideo.privacy` is the actual server
response, including a different returned status. Current Google documentation
says unverified API projects are not restricted to Private; account settings can
still affect defaults. Code does not assume a project-specific privacy rule.

## Native panel

Open **Project → YouTube…** to use the native panel implemented in
`apps/sidebar-video/src/youtube.rs`. Opening it or
changing projects does not construct a client, read the keychain or start HTTP.
Connection, Upload MP4 and Import clip views separate the explicit actions.
The Load saved connection and Desktop JSON picker run on workers; Connect opens
only its private authorization URL in the system browser after the user's click.

Upload review binds the displayed connection to the chosen MP4/title/privacy.
Editing publishing fields clears that confirmation. A pending upload has its own
review and explicit Resume/Forget controls. The displayed completed privacy comes
from the server. Primary Publish, Resume and Import actions sit outside scrolling
form content. Import shows its selected byte/time budgets and actual written
bytes; it does not present the byte budget as a completion percentage.

One bounded completion channel and a latest-progress slot serve each operation.
The project navigation guard is held only while that operation is pending, then
released on success, error, cancellation, panic or channel loss. Workers never
replace an Editor on acknowledgement. Imported media queues through the existing
capture append/autosave path; newer editor/form drafts stay intact. Project
changes cancel and reject earlier project-bound results, clear file/review data,
and require another explicit connection load.

If an import finishes after cancellation, its already-published source stays on
disk without joining the sequence; the panel explains how to add it later. An
upload completion remains visible after a late cancellation because the remote
video may already exist.

The panel is integrated with the Project menu, polling, navigation guards and
existing capture import queue. Compilation and all seven local panel fixtures
passed with the final workspace checks below. Native modal screenshots and real
Google consent/upload remain unverified.

## Credentials and process coordination

Client secrets, tokens, session URI and resumable metadata are stored only through
native keyring backends: synchronous Secret Service on Linux, Windows Credential
Manager and the macOS keychain in their respective target configurations. A
missing or locked store returns an actionable error; there is no credential-file
or mock-backend fallback in production. Tests inject a separate memory store.

A record is bounded to 32 KiB. Two fixed slots contain at most 2,000-byte entries;
UTF-8 boundaries are preserved. Whole-record length/hash and generation checks
refuse corrupt/mixed chunks before JSON parsing. The small active manifest is
written last, retaining the previous complete slot if staging fails. Serialized
buffers use `Zeroizing`; this is not a promise that every transient allocation
is erased. Client configuration, token and authorization Debug output is redacted.

Normal app processes coordinate through the existing Catalyst advisory-file lock
in the per-user Storyfold state namespace. The lock file contains no credentials
and the OS releases ownership after a crash. Its directory helper rejects
traversal, linked/reparse folders, foreign owners and unsafe Unix write modes.
Creation happens only on explicit operations. This coordinates normal instances;
it does not resist malicious code already running as the same user.

## Upload bounds and recovery

Publish accepts an existing normal `exports/*.mp4` file confined by Catalyst's
shared helpers. It checks an MP4 container header and the existing bounded media
probe, binds metadata and the complete SHA256/source fingerprint, and leaves the
local MP4 and project JSON unchanged. Links, playlists and stale sources fail.

HTTPS uses fixed Google endpoints and disabled redirects/environment proxies.
PKCE uses S256 and fresh random state; the callback is limited to 127.0.0.1 with
an ephemeral port, matching GET/path/Host/state, bounded headers and duplicate
refusal. The callback wait has a 180-second deadline. Token and upload requests
have 45-second request and 5-second connection bounds; cancellation is checked
between requests/chunks, so an in-flight request can finish before cancellation.

Uploads use 1 MiB chunks. Progress comes from server-acknowledged308 Range prefixes,
never bytes merely sent or a timer. Interruption triggers bounded backoff and a
status query before more bytes. Explicit Resume checks the same session/source;
a pending session blocks accidental new uploads. Source changes, expiration,
permission/quota errors and cancellation preserve the local MP4. Unknown final
completion remains a status-recoverable session rather than an automatic new POST.
Title is limited to 100 Unicode characters and description to 5,000 UTF-8 bytes.

## Public clip import

Linux `import_clip` requires one direct HTTPS YouTube video link and explicit
start/end times. Watch, Shorts, live-video and youtu.be forms canonicalize to one
video ID. Playlists, redirect wrappers, credentials, ports and malformed ranges
are refused. The UI must display the selected byte/time budgets: nonzero and at
most 4 GiB/30 minutes per operation. `written_bytes` reports actual current private
output size, which can change during merge/rename; it is not a percentage.

FFmpeg must be installed. Packaged builds prefer the app-owned yt-dlp tool at
`<prefix>/share/storyfold-record/tools/yt-dlp`. The installer verifies the pinned
2026.08.19 official executable SHA256 before copying it, and preserves upstream
license notices. Invalid existing bundled files fail rather than silently using
another executable. Development builds without a bundle can use a compatible
system yt-dlp; Ubuntu24.04's 2024.04.09 rejects the required plugin/remote-component
isolation flags and cannot run this importer. The system copy is left unchanged. Commands use
fixed arguments, direct HTTPS MP4/M4A formats, private fixed output, no shell,
config/plugins/cookies/remote-component loading, and one configured public-only
HTTPS proxy shared with the existing AI evidence code. FFmpeg inputs also receive
an explicit proxy/protocol/MOV allowlist; no direct-network fallback is enabled.
The environment is cleared before setting fixed tool/proxy/temp values.

The Linux supervisor preserves an exited leader until its group is stopped,
then reaps it; cancel/failure cannot leave the downloader's FFmpeg descendants
running. Output/diagnostics, file count, byte budget and deadline are bounded.
A validated completed MP4 is atomically imported through existing media helpers;
its bytes are preserved and `ImportedMedia::append_to` performs the timeline
transaction. Temporary output is removed. Non-Linux import returns an explicit
unsupported error until a safe process supervisor exists for that target.

## Checks and acceptance limits

On 2026-10-20:

- Final full-workspace tests passed **123 tests**, with **22 opt-in tests
  ignored**: app 23, video core 83, shared core 8, workflow 8 and Catalyst UI 1.
  All-target, all-feature Clippy passed with warnings denied.
- The seven native YouTube panel fixtures passed without keychain, browser,
  network or camera actions. They cover idle credential access, review binding,
  actual returned privacy and newer drafts, worker-channel loss, late-canceled
  imports, stale project results, minimum-window control bounds and a real
  pointer click on the review checkbox. The pointer fixture waits for the
  centered modal's measured position before using its accessibility bounds.
- 16 pure/fake/local-callback tests passed in 3.75 s: PKCE vector, client shape,
  redacted Debug, state/path/method/Host/duplicates/body, refresh/refusal, idle
  construction, private chunks/Unicode/corruption/staged failure, byte ranges,
  retries/partial acknowledgement/zero-byte resume/completion, cancellation and
  changed-source preservation, Unlisted default and actual returned Private.
- Secure state helper tests passed 2/2, including separate real child processes
  contending for/releasing an empty OS lock. Missing folders, traversal, links,
  foreign Unix ownership and writable leaves were exercised with private fixtures.
  HOME and global umask were not modified. Initial default tempfile permissions
  were correctly refused; fixtures now request 0700 when created.
- Public proxy tests passed 3/3, including the coordinator's shared transfer bound.
- Explicit `STORYFOLD_CAPTURE_TEST=1` generated integrations passed 2/2 in 1.16 s.
  A real generated MP4 crossed a 1 MiB cancellation boundary, resumed through the
  public API/fake server, matched remaining bytes exactly and reported actual
  Private while preserving MP4/JSON. A fake installed downloader copied a real
  generated MP4 exactly, enforced the byte budget, killed its forked child on
  cancel and left no work directory. No Google/public-media request occurred.

The Linux adapter compiled and the native panel's local interaction tests passed,
but no native modal screenshot, real host-keychain read/write, Google client/account
access, consent/upload, real public YouTube download or Windows/macOS runtime check
has been performed. Those are remaining acceptance, not inferred successes.

## Primary references

Checked 2026-10-20:

- https://developers.google.com/youtube/data/guides/auth/installed-apps
- https://developers.google.com/youtube/data/guides/using_resumable_upload_protocol
- https://developers.google.com/youtube/data/reference/rest/v3/videos/insert
- https://developers.google.com/youtube/data/reference/rest/v3/videos
- https://docs.rs/crate/keyring/3.6.3
- https://docs.rs/crate/keyring/3.6.3/source/Cargo.toml
- https://github.com/yt-dlp/yt-dlp
- https://github.com/yt-dlp/yt-dlp/blob/master/yt_dlp/downloader/external.py
