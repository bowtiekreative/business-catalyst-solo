"""Generated-only capture-path clock benchmark; no devices, GUI or network."""
import argparse
import json
import os
from pathlib import Path
import re
import selectors
import signal
import statistics
import subprocess
import tempfile
import time

FFMPEG = "/usr/bin/ffmpeg"
FRAME_BYTES = 1280 * 720 * 4
SOURCE = (
    "testsrc2=size=1920x1080:rate=30,"
    "drawbox=x=0:y=0:w=1100:h=150:color=white:t=fill,"
    r"drawtext=font=monospace:text='%{localtime\:%s.%3N}':fontsize=90:fontcolor=black:x=24:y=24"
)
FILTER_CLOCK = (
    "drawbox=x=0:y=180:w=1100:h=150:color=white:t=fill,"
    r"drawtext=font=monospace:text='%{localtime\:%s.%3N}':fontsize=90:fontcolor=black:x=24:y=204,"
)
COMMON = [FFMPEG, "-hide_banner", "-loglevel", "error", "-nostats", "-nostdin", "-n"]


def spawn(args, **kwargs):
    return subprocess.Popen(args, start_new_session=True, **kwargs)


def stop(child):
    if child is None:
        return
    try:
        os.killpg(child.pid, signal.SIGTERM)
    except ProcessLookupError:
        pass
    try:
        child.wait(timeout=3)
    except subprocess.TimeoutExpired:
        os.killpg(child.pid, signal.SIGKILL)
        child.wait(timeout=3)


def clock_crop(frame, top=0):
    rgba = b"".join(frame[y * 1280 * 4:y * 1280 * 4 + 740 * 4] for y in range(top, top + 100))
    rgb = bytearray(740 * 100 * 3)
    for channel in range(3):
        rgb[channel::3] = rgba[channel::4]
    return b"P6\n740 100\n255\n" + rgb


def percentile(values, fraction):
    ordered = sorted(values)
    return round(ordered[min(int(len(ordered) * fraction), len(ordered) - 1)], 2)


def ocr(samples):
    clocks, upstream = ([], []), []
    for received, source_crop, filter_crop in samples:
        pair = []
        for crop, ages in zip((source_crop, filter_crop), clocks):
            result = subprocess.run(
                ["/usr/bin/tesseract", "stdin", "stdout", "--psm", "7", "-c", "tessedit_char_whitelist=0123456789."],
                input=crop, capture_output=True, timeout=4,
                env={**os.environ, "OMP_THREAD_LIMIT": "1"},
            )
            text = result.stdout.decode().strip()
            age = (received - float(text)) * 1000 if result.returncode == 0 and re.fullmatch(r"\d{10}\.\d{3}", text) else None
            if age is not None and -25 <= age < 10000:
                ages.append(age)
                pair.append(age)
        if len(pair) == 2:
            upstream.append(pair[0] - pair[1])
    proof = {"attempted": len(samples)}
    for name, ages in zip(("source_clock", "filter_clock"), clocks):
        assert len(ages) >= max(8, len(samples) * .8), f"{name} OCR failed: {len(ages)}/{len(samples)}"
        proof[name] = {"samples": len(ages), "age_ms": {
            "min": round(min(ages), 2), "median": round(statistics.median(ages), 2),
            "p95": percentile(ages, .95), "max": round(max(ages), 2),
        }}
    proof["source_to_filter_ms_median"] = round(statistics.median(upstream), 2)
    return proof


def benchmark(source, variant, seconds, directory):
    producer = capture = None
    error_log = directory / "ffmpeg.log"
    no_fps = variant in ("no-fps", "preview-first-no-fps")
    first = variant in ("preview-first", "preview-first-no-fps")
    decode_only = variant == "decode-only"
    args = COMMON + ["-filter_complex_threads", "1"]
    if source == "lavfi":
        args += ["-re", "-thread_queue_size", "2", "-f", "lavfi", "-i", SOURCE]
    else:
        # Known MJPEG/framerate simulates compressed camera decoding without
        # filesystem/network probing or any physical capture device.
        args += ["-thread_queue_size", "2", "-analyzeduration", "0", "-probesize", "32", "-f", "mjpeg", "-framerate", "30", "-i", "pipe:0"]
    if not decode_only:
        args += ["-re", "-thread_queue_size", "8", "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000"]
    fps = "" if no_fps else "fps=30,"
    graph = f"[0:v]{FILTER_CLOCK}{fps}setpts=N/(30*TB),split=2[vrec][vp];[vp]scale=1280:720,format=rgba[preview];[1:a]asetpts=PTS-STARTPTS,aresample=async=1:first_pts=0[a]"
    if decode_only:
        graph = f"[0:v]{FILTER_CLOCK}scale=1280:720,format=rgba[preview]"
    args += ["-filter_complex", graph]
    recording = [
        "-map", "[vrec]", "-map", "[a]", "-t", str(seconds), "-c:v", "libx264", "-threads", "2",
        "-preset", "veryfast", "-tune", "zerolatency", "-crf", "20", "-pix_fmt", "yuv420p",
        "-profile:v", "high", "-g", "30", "-keyint_min", "30", "-sc_threshold", "0", "-bf", "0",
        "-c:a", "aac", "-b:a", "160k", "-ar", "48000", "-ac", "2", "-f", "segment",
        "-segment_format", "mpegts", "-segment_time", "1", "-segment_time_delta", "0.016667",
        "-reset_timestamps", "1", "-segment_list", "segments.csv", "-segment_list_type", "csv",
        "-segment_list_size", "0", "segment-%09d.ts",
    ]
    preview = ["-map", "[preview]", "-t", str(seconds), "-an", "-c:v", "rawvideo", "-threads", "1", "-pix_fmt", "rgba"]
    if no_fps:
        preview += ["-fps_mode", "passthrough"]
    preview += ["-f", "rawvideo", "pipe:1"]
    args += preview if decode_only else (preview + recording if first else recording + preview)
    with error_log.open("wb") as errors:
        try:
            if source == "mjpeg":
                producer = spawn(
                    COMMON + ["-filter_threads", "1", "-re", "-f", "lavfi", "-i", SOURCE,
                              "-t", str(seconds + 3), "-an", "-c:v", "mjpeg", "-threads", "1",
                              "-q:v", "3", "-pix_fmt", "yuvj420p", "-f", "mjpeg", "pipe:1"],
                    stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=errors,
                )
            started = time.monotonic()
            capture = spawn(args, cwd=directory, stdin=producer.stdout if producer else subprocess.DEVNULL,
                            stdout=subprocess.PIPE, stderr=errors, bufsize=0)
            if producer:
                producer.stdout.close()
            frames, samples, times, buffer = 0, [], [], bytearray()
            with selectors.DefaultSelector() as selector:
                selector.register(capture.stdout, selectors.EVENT_READ)
                while time.monotonic() - started < seconds + 12:
                    if not selector.select(.3):
                        continue
                    chunk = os.read(capture.stdout.fileno(), 1024 * 1024)
                    if not chunk:
                        break
                    buffer.extend(chunk)
                    while len(buffer) >= FRAME_BYTES:
                        frame = bytes(buffer[:FRAME_BYTES])
                        del buffer[:FRAME_BYTES]
                        received = time.time()
                        frames += 1
                        if frames > 30:
                            times.append(time.monotonic())
                            if frames % 6 == 0:
                                samples.append((frames, received, clock_crop(frame), clock_crop(frame, 120)))
                else:
                    raise AssertionError("Generated capture exceeded bounded timeout")
            capture.wait(timeout=4)
            assert capture.returncode == 0, error_log.read_text()[-1500:]
            assert len(times) >= 60 and len(samples) >= 10, f"Insufficient frames: {frames}"
            # Exclude one-second startup and final half-second EOF drain.
            times = times[:-15]
            samples = [sample[1:] for sample in samples if sample[0] <= frames - 15]
            fps = (len(times) - 1) / (times[-1] - times[0])
            intervals = [(b - a) * 1000 for a, b in zip(times, times[1:])]
            result = {"source": source, "variant": variant, "recording_branch": not decode_only, "frames": frames,
                      "steady_fps": round(fps, 2), "pipe_interval_ms_p95": percentile(intervals, .95)}
        finally:
            stop(capture)
            stop(producer)
    # OCR is intentionally after draining/reaping capture, never in its reader.
    result.update(ocr(samples))
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--seconds", type=int, choices=range(4, 11), default=6)
    parser.add_argument("--source", choices=("both", "lavfi", "mjpeg"), default="both")
    parser.add_argument("--variant", choices=("all", "isolation", "baseline", "no-fps", "preview-first", "preview-first-no-fps", "decode-only"), default="all")
    args = parser.parse_args()
    assert os.environ.get("STORYFOLD_PREVIEW_BENCHMARK") == "1", "Explicit generated benchmark flag required"
    sources = ("lavfi", "mjpeg") if args.source == "both" else (args.source,)
    variants = ("baseline", "no-fps", "preview-first", "preview-first-no-fps") if args.variant == "all" else (args.variant,)
    if args.variant == "isolation":
        variants = ("baseline", "decode-only")
    proof = {"synthetic_only": True, "physical_devices_opened": False,
             "capture": "1920x1080@30 H264 x264 veryfast zerolatency CRF20 two threads +AAC160k one-second TS segments",
             "preview": "1280x720 RGBA pipe", "competing_load": "Unthrottled generated1920x1080@30 x264 veryfast two threads",
             "clock_location": "lavfi source before capture filters; MJPEG mode also before source MJPEG encoding/pipe/decoding",
             "second_clock": "Capture filter entry after decoding, before fps normalization/scaling/output",
             "steady_window": "Exclude first30frames and final15frames to avoid startup/EOF drain",
             "mjpeg_fixture_limit": "Separate software MJPEG producer and compressed pipe can buffer/backpressure differently from V4L2; source age is not a physical camera measurement",
             "excludes": ["camera sensor/exposure", "USB/V4L2 driver", "application texture upload", "GPU/UI paint"],
             "ffmpeg": subprocess.run([FFMPEG, "-version"], capture_output=True, text=True, check=True).stdout.splitlines()[0], "results": []}
    with tempfile.TemporaryDirectory(prefix="sf-preview-benchmark-") as temporary:
        load = spawn(COMMON + ["-filter_threads", "1", "-f", "lavfi", "-i", "testsrc2=size=1920x1080:rate=30",
                               "-an", "-c:v", "libx264", "-threads", "2", "-preset", "veryfast", "-f", "null", "-"],
                     stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        try:
            for source in sources:
                for variant in variants:
                    directory = Path(temporary) / f"{source}-{variant}"
                    directory.mkdir()
                    result = benchmark(source, variant, args.seconds, directory)
                    assert load.poll() is None, "Competing encoder exited"
                    proof["results"].append(result)
                    print(json.dumps(result), flush=True)
        finally:
            stop(load)
    suffix = "-isolation" if args.variant == "isolation" else ""
    output = Path(__file__).resolve().parent.parent / f"data/record-studio-input/native-preview-benchmark{suffix}.json"
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(proof, indent=2) + "\n")


if __name__ == "__main__":
    main()
