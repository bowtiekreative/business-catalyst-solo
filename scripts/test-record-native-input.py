"""Own-app OS event fixture. Requires fresh xvfb-run, never the desktop."""
import argparse
import ctypes as c
import json
import os
from pathlib import Path
import re
import signal
import stat
import subprocess
import tempfile
import time


def require_fixture():
    assert os.environ.get("STORYFOLD_INPUT_FIXTURE") == "1", "Fixture flag required"
    name = os.environ.get("DISPLAY", "")
    assert re.fullmatch(r":[0-9]+(?:\.[0-9]+)?", name), "Local Xvfb display required"
    authority = Path(os.environ.get("XAUTHORITY", ""))
    assert authority.is_absolute() and authority.parent.name.startswith("xvfb-run."), "Fresh xvfb-run authority required"
    info = authority.lstat()
    assert stat.S_ISREG(info.st_mode) and info.st_uid == os.getuid(), "Owned authority file required"
    assert info.st_mode & 0o077 == 0, "Private authority file required"
    for process in Path("/proc").iterdir():
        if not process.name.isdigit():
            continue
        try:
            if process.stat().st_uid != os.getuid() or (process / "comm").read_text().strip() != "Xvfb":
                continue
            args = (process / "cmdline").read_bytes().rstrip(b"\0").split(b"\0")
            if os.fsencode(name.split(".")[0]) in args and b"-auth" in args:
                auth = args[args.index(b"-auth") + 1]
                if Path(os.fsdecode(auth)).resolve() == authority.resolve():
                    return
        except (OSError, IndexError):
            continue
    raise AssertionError("Authority/display must belong to a live owned Xvfb server")


class Attributes(c.Structure):
    _fields_ = [(name, c.c_int) for name in ("x", "y", "width", "height", "border_width", "depth")] + [
        ("visual", c.c_void_p), ("root", c.c_ulong), ("window_class", c.c_int),
        ("bit_gravity", c.c_int), ("win_gravity", c.c_int), ("backing_store", c.c_int),
        ("backing_planes", c.c_ulong), ("backing_pixel", c.c_ulong), ("save_under", c.c_int),
        ("colormap", c.c_ulong), ("map_installed", c.c_int), ("map_state", c.c_int),
        ("all_event_masks", c.c_long), ("your_event_mask", c.c_long),
        ("do_not_propagate_mask", c.c_long), ("override_redirect", c.c_int), ("screen", c.c_void_p),
    ]


def records(path):
    rows = []
    for line in path.read_text().splitlines():
        try:
            row = json.loads(line)
            if isinstance(row, dict) and row.get("probe") == "storyfold-record-input":
                rows.append(row)
        except json.JSONDecodeError:
            pass
    return rows


def run(record, minimum):
    root = Path(__file__).resolve().parent.parent
    x, xt = c.CDLL("libX11.so.6"), c.CDLL("libXtst.so.6")
    display, window = c.c_void_p, c.c_ulong
    x.XOpenDisplay.argtypes, x.XOpenDisplay.restype = [c.c_char_p], display
    x.XDefaultRootWindow.argtypes, x.XDefaultRootWindow.restype = [display], window
    x.XQueryTree.argtypes = [display, window, c.POINTER(window), c.POINTER(window), c.POINTER(c.POINTER(window)), c.POINTER(c.c_uint)]
    x.XFetchName.argtypes = [display, window, c.POINTER(c.c_void_p)]
    x.XFree.argtypes = [c.c_void_p]
    x.XInternAtom.argtypes, x.XInternAtom.restype = [display, c.c_char_p, c.c_int], c.c_ulong
    x.XGetWindowProperty.argtypes = [display, window, c.c_ulong, c.c_long, c.c_long, c.c_int, c.c_ulong, c.POINTER(c.c_ulong), c.POINTER(c.c_int), c.POINTER(c.c_ulong), c.POINTER(c.c_ulong), c.POINTER(c.POINTER(c.c_ubyte))]
    x.XGetWindowAttributes.argtypes = [display, window, c.POINTER(Attributes)]
    x.XRaiseWindow.argtypes = [display, window]
    x.XSetInputFocus.argtypes = [display, window, c.c_int, c.c_ulong]
    x.XGetInputFocus.argtypes = [display, c.POINTER(window), c.POINTER(c.c_int)]
    x.XTranslateCoordinates.argtypes = [display, window, window, c.c_int, c.c_int, c.POINTER(c.c_int), c.POINTER(c.c_int), c.POINTER(window)]
    x.XSync.argtypes, x.XCloseDisplay.argtypes = [display, c.c_int], [display]
    xt.XTestFakeMotionEvent.argtypes = [display, c.c_int, c.c_int, c.c_int, c.c_ulong]
    xt.XTestFakeButtonEvent.argtypes = [display, c.c_uint, c.c_int, c.c_ulong]
    error_type = c.CFUNCTYPE(c.c_int, display, c.c_void_p)
    ignore_raced_window = error_type(lambda _display, _error: 0)
    x.XSetErrorHandler.argtypes = [error_type]
    x.XSetErrorHandler(ignore_raced_window)

    with tempfile.TemporaryDirectory(prefix="sf-native-input-") as temporary:
        log = Path(temporary) / "probe.log"
        env = os.environ.copy()
        for key in list(env):
            if key.startswith("SIDEBAR_VIDEO_VALIDATION_") or key in (
                "EFRAME_SCREENSHOT_TO", "WAYLAND_DISPLAY", "WAYLAND_SOCKET",
                "STORYFOLD_RECORD_GENERATED_FIXTURE", "STORYFOLD_CAPTURE_TEST",
            ):
                env.pop(key)
        env["STORYFOLD_RECORD_INPUT_PROBE"] = "1"
        env["XDG_SESSION_TYPE"] = "x11"
        if record:
            env["STORYFOLD_CAPTURE_TEST"] = "1"
            env["STORYFOLD_RECORD_GENERATED_FIXTURE"] = str(Path(temporary) / "project")
        if minimum:
            env["SIDEBAR_VIDEO_VALIDATION_MIN"] = "1"
        with log.open("w") as stream:
            app = subprocess.Popen(
                [str(root / "target/debug/sidebar-video")], env=env,
                stdin=subprocess.DEVNULL, stdout=stream, stderr=stream,
                start_new_session=True,
            )
        connection = None
        try:
            connection = x.XOpenDisplay(None)
            assert connection, "Cannot open owned Xvfb"
            desktop = x.XDefaultRootWindow(connection)
            pid_atom = x.XInternAtom(connection, b"_NET_WM_PID", 0)
            cardinal = x.XInternAtom(connection, b"CARDINAL", 0)

            def owned(win):
                kind, count, remaining, fmt = c.c_ulong(), c.c_ulong(), c.c_ulong(), c.c_int()
                data = c.POINTER(c.c_ubyte)()
                result = x.XGetWindowProperty(connection, win, pid_atom, 0, 1, 0, cardinal, c.byref(kind), c.byref(fmt), c.byref(count), c.byref(remaining), c.byref(data))
                try:
                    matches = result == 0 and kind.value == cardinal and fmt.value == 32 and count.value == 1 and bool(data) and c.cast(data, c.POINTER(c.c_ulong))[0] == app.pid
                finally:
                    if data:
                        x.XFree(data)
                if not matches:
                    return False
                name = c.c_void_p()
                if not x.XFetchName(connection, win, c.byref(name)) or not name.value:
                    return False
                try:
                    title = c.string_at(name).decode(errors="replace")
                    return title.startswith("Storyfold · Record studio ") and title.endswith(" · Input check")
                finally:
                    x.XFree(name)

            def wait(predicate, timeout=15):
                deadline = time.monotonic() + timeout
                while time.monotonic() < deadline:
                    assert app.poll() is None, "Owned fixture application exited"
                    rows = records(log)
                    if rows and predicate(rows[-1]):
                        return rows[-1]
                    time.sleep(.03)
                raise AssertionError(f"State timeout: {records(log)[-3:]}")

            deadline, target, attributes = time.monotonic() + 15, None, Attributes()
            while time.monotonic() < deadline and target is None:
                assert app.poll() is None, "Owned fixture application exited before mapping"
                queue = [(desktop, 0)]
                while queue:
                    parent, depth = queue.pop()
                    a, b, count, children = window(), window(), c.c_uint(), c.POINTER(window)()
                    if not x.XQueryTree(connection, parent, c.byref(a), c.byref(b), c.byref(children), c.byref(count)):
                        continue
                    try:
                        assert count.value <= 4096, "Unexpected fixture window tree"
                        for index in range(count.value):
                            win = int(children[index])
                            if owned(win) and x.XGetWindowAttributes(connection, win, c.byref(attributes)) and attributes.map_state == 2:
                                target = win
                                break
                            if depth < 3:
                                queue.append((win, depth + 1))
                    finally:
                        if children:
                            x.XFree(children)
                    if target is not None:
                        break
                if target is None:
                    time.sleep(.05)
            assert target is not None, "Owned PID/title window did not become viewable"
            x.XRaiseWindow(connection, target)
            x.XSetInputFocus(connection, target, 2, 0)
            x.XSync(connection, 0)

            def focus_owned():
                assert app.poll() is None and owned(target), "Fixture target lost ownership"
                focus, revert = window(), c.c_int()
                x.XGetInputFocus(connection, c.byref(focus), c.byref(revert))
                assert focus.value == target, "Focus left owned fixture"

            def move(control=None):
                focus_owned()
                assert x.XGetWindowAttributes(connection, target, c.byref(attributes)) and attributes.map_state == 2
                px, py, child = c.c_int(), c.c_int(), window()
                assert x.XTranslateCoordinates(connection, target, desktop, 0, 0, c.byref(px), c.byref(py), c.byref(child))
                if control:
                    row = wait(lambda row: row.get(control, {}).get("enabled", False))
                    bounds, scale = row[control]["rect"], row["pixels_per_point"]
                    local_x, local_y = (bounds[0] + bounds[2]) * scale / 2, (bounds[1] + bounds[3]) * scale / 2
                else:
                    local_x, local_y = 10, 140  # Release outside the Record control.
                assert 0 <= local_x < attributes.width and 0 <= local_y < attributes.height
                assert xt.XTestFakeMotionEvent(connection, -1, px.value + int(local_x), py.value + int(local_y), 0)
                x.XSync(connection, 0)
                time.sleep(.12)

            def button(pressed):
                focus_owned()
                assert xt.XTestFakeButtonEvent(connection, 1, int(pressed), 0)
                x.XSync(connection, 0)

            def tap(control="record_control"):
                move(control)
                button(True)
                time.sleep(.08)
                button(False)

            result = {"isolated_xvfb": True, "real_x11_xtest_events": True, "matched_child_pid": True}
            if record:
                wait(lambda row: row.get("preview_present") and row.get("can_record") and row.get("record_control", {}).get("enabled"), 20)
                assert records(log)[-1]["clip_count"] == 0
                started = time.monotonic()
                move("record_control")
                button(True)
                wait(lambda row: row.get("taking") and row.get("record_control", {}).get("down"), 3)
                time.sleep(.6)
                move()
                button(False)
                wait(lambda row: not row.get("taking") and row.get("clip_count") == 1)
                held_ms = round((time.monotonic() - started) * 1000)
                tap()
                wait(lambda row: row.get("taking"), 3)
                time.sleep(.55)
                tap()
                wait(lambda row: not row.get("taking") and row.get("clip_count") == 2)
                rows = records(log)
                assert any(row.get("record_control", {}).get("clicked") for row in rows)
                for taking, clip_count in ((True, 2), (False, 3)):
                    before = records(log)[-1]["sample"]
                    tap("record_toggle")
                    wait(lambda row: row.get("taking") == taking and row.get("clip_count") == clip_count)
                    assert any(
                        row["sample"] > before and (row.get("record_toggle") or {}).get("clicked")
                        for row in records(log)
                    ), "Normal recording button did not receive a clicked response"
                    if taking:
                        time.sleep(.55)
                rows = records(log)
                assert sum(row.get("presses", 0) for row in rows) >= 5
                assert sum(row.get("releases", 0) for row in rows) >= 5
                result.update({"size": "minimum1160x800" if minimum else "default1480x1000", "generated_source_only": True, "live_preview_present": True, "record_pointer_down_received": True, "held_release_outside_saved_clip": True, "tap_started_long_take": True, "second_tap_saved_long_take": True, "normal_button_started_recording": True, "normal_button_stopped_and_saved": True, "normal_button_clicked_responses": 2, "saved_clip_count": 3, "first_clip_ready_ms": held_ms})
                wait(lambda row: not row.get("project_busy") and not row.get("capture_busy"))
                project = Path(temporary) / "project"
                saved_json = (project / "project.json").read_bytes()
                exports = sorted(path.name for path in (project / "exports").iterdir())
                before = records(log)[-1]["sample"]
                tap("preview_control")
                wait(lambda row: row.get("sequence_preview_open") and not row.get("output_busy") and not row.get("camera_running"), 20)
                rows = [row for row in records(log) if row["sample"] > before]
                assert any((row.get("preview_control") or {}).get("clicked") for row in rows), "Preview did not receive the OS click"
                assert not any(row.get("export_pending") for row in rows), "Preview started a full export"
                assert sorted(path.name for path in (project / "exports").iterdir()) == exports, "Preview created an export"
                assert (project / "project.json").read_bytes() == saved_json, "Preview changed saved edits"
                result.update({"native_preview_clicked": True, "timeline_player_opened": True, "preview_stopped_camera": True, "preview_did_not_export_or_save": True})
            else:
                move("project_menu")
                button(True)
                time.sleep(.15)
                button(False)
                wait(lambda row: row.get("project_menu", {}).get("popup_open"))
                rows = records(log)
                assert any(row.get("pointer_buttons", 0) for row in rows)
                result.update({"native_pointer_button_received": True, "project_menu_opens": True})
            result["probe_records"] = records(log)
            return result
        finally:
            # The app and generated FFmpeg children share this new fixture-only
            # process group. Never target pre-existing application processes.
            try:
                os.killpg(app.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            try:
                app.wait(timeout=5)
            except subprocess.TimeoutExpired:
                os.killpg(app.pid, signal.SIGKILL)
                app.wait(timeout=5)
            # Reap any generated child still draining after its parent exited.
            try:
                os.killpg(app.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            if connection:
                x.XCloseDisplay(connection)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--record", action="store_true", help="Verify generated live Record at default and minimum window sizes")
    args = parser.parse_args()
    require_fixture()
    results = [run(True, minimum) for minimum in (False, True)] if args.record else [run(False, False)]
    root = Path(__file__).resolve().parent.parent
    output = root / "data/record-studio-input"
    output.mkdir(parents=True, exist_ok=True)
    artifact = output / ("native-os-record-input.json" if args.record else "native-os-input.json")
    proof = {"runs": results} if args.record else results[0]
    artifact.write_text(json.dumps(proof, indent=2) + "\n")
    print(json.dumps([{k: v for k, v in result.items() if k != "probe_records"} for result in results]))
