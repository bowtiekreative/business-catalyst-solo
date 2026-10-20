"""Fixed mpv control actions on an owned child in fresh Xvfb; never a desktop."""
import argparse
import ctypes as c
import json
import os
from pathlib import Path
import re
import stat
import time

TITLE = "Storyfold generated native input fixture"


def require_fixture(pid):
    assert os.environ.get("STORYFOLD_INPUT_FIXTURE") == "1", "Fixture flag required"
    display = os.environ.get("DISPLAY", "")
    assert re.fullmatch(r":[0-9]+(?:\.[0-9]+)?", display), "Local Xvfb display required"
    authority = Path(os.environ.get("XAUTHORITY", ""))
    assert authority.is_absolute() and authority.parent.name.startswith("xvfb-run."), "Fresh xvfb-run authority required"
    info = authority.lstat()
    assert stat.S_ISREG(info.st_mode) and info.st_uid == os.getuid(), "Owned authority file required"
    assert info.st_mode & 0o077 == 0, "Private authority file required"
    server_display = display.split(".")[0]
    matched = False
    for process in Path("/proc").iterdir():
        if not process.name.isdigit():
            continue
        try:
            if process.stat().st_uid != os.getuid() or (process / "comm").read_text().strip() != "Xvfb":
                continue
            args = (process / "cmdline").read_bytes().rstrip(b"\0").split(b"\0")
            if os.fsencode(server_display) not in args or b"-auth" not in args:
                continue
            auth = args[args.index(b"-auth") + 1]
            if Path(os.fsdecode(auth)).resolve() == authority.resolve():
                matched = True
                break
        except (OSError, IndexError):
            continue
    assert matched, "Authority/display must belong to a live owned Xvfb server"
    child = Path("/proc") / str(pid)
    assert child.stat().st_uid == os.getuid(), "Owned player required"
    assert (child / "comm").read_text().strip() == "mpv", "Only the fixture player is accepted"
    parent = next(line.split()[1] for line in (child / "status").read_text().splitlines() if line.startswith("PPid:"))
    assert int(parent) == os.getppid(), "Player must be a direct child of the calling Rust test"


class Attributes(c.Structure):
    _fields_ = [(name, c.c_int) for name in ("x", "y", "width", "height", "border_width", "depth")] + [
        ("visual", c.c_void_p), ("root", c.c_ulong), ("window_class", c.c_int),
        ("bit_gravity", c.c_int), ("win_gravity", c.c_int), ("backing_store", c.c_int),
        ("backing_planes", c.c_ulong), ("backing_pixel", c.c_ulong), ("save_under", c.c_int),
        ("colormap", c.c_ulong), ("map_installed", c.c_int), ("map_state", c.c_int),
        ("all_event_masks", c.c_long), ("your_event_mask", c.c_long),
        ("do_not_propagate_mask", c.c_long), ("override_redirect", c.c_int), ("screen", c.c_void_p),
    ]


def act(pid, action):
    require_fixture(pid)
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
    x.XTranslateCoordinates.argtypes = [display, window, window, c.c_int, c.c_int, c.POINTER(c.c_int), c.POINTER(c.c_int), c.POINTER(window)]
    x.XRaiseWindow.argtypes = [display, window]
    x.XSetInputFocus.argtypes = [display, window, c.c_int, c.c_ulong]
    x.XGetInputFocus.argtypes = [display, c.POINTER(window), c.POINTER(c.c_int)]
    x.XStringToKeysym.argtypes, x.XStringToKeysym.restype = [c.c_char_p], c.c_ulong
    x.XKeysymToKeycode.argtypes, x.XKeysymToKeycode.restype = [display, c.c_ulong], c.c_ubyte
    x.XSync.argtypes, x.XCloseDisplay.argtypes = [display, c.c_int], [display]
    xt.XTestFakeMotionEvent.argtypes = [display, c.c_int, c.c_int, c.c_int, c.c_ulong]
    xt.XTestFakeButtonEvent.argtypes = [display, c.c_uint, c.c_int, c.c_ulong]
    xt.XTestFakeKeyEvent.argtypes = [display, c.c_uint, c.c_int, c.c_ulong]
    error_type = c.CFUNCTYPE(c.c_int, display, c.c_void_p)
    ignore_raced_window = error_type(lambda _display, _error: 0)
    x.XSetErrorHandler.argtypes = [error_type]
    x.XSetErrorHandler(ignore_raced_window)
    connection = x.XOpenDisplay(None)
    assert connection, "Cannot open the owned Xvfb display"
    try:
        root = x.XDefaultRootWindow(connection)
        pid_atom = x.XInternAtom(connection, b"_NET_WM_PID", 0)
        cardinal = x.XInternAtom(connection, b"CARDINAL", 0)

        def owned(win):
            kind, count, remaining, fmt = c.c_ulong(), c.c_ulong(), c.c_ulong(), c.c_int()
            data = c.POINTER(c.c_ubyte)()
            result = x.XGetWindowProperty(connection, win, pid_atom, 0, 1, 0, cardinal, c.byref(kind), c.byref(fmt), c.byref(count), c.byref(remaining), c.byref(data))
            try:
                matches = result == 0 and kind.value == cardinal and fmt.value == 32 and count.value == 1 and bool(data) and c.cast(data, c.POINTER(c.c_ulong))[0] == pid
            finally:
                if data:
                    x.XFree(data)
            if not matches:
                return False
            name = c.c_void_p()
            if not x.XFetchName(connection, win, c.byref(name)) or not name.value:
                return False
            try:
                return c.string_at(name).decode(errors="replace") == TITLE
            finally:
                x.XFree(name)

        deadline, target, attributes = time.monotonic() + 5, None, Attributes()
        while time.monotonic() < deadline and target is None:
            queue = [(root, 0)]
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
                time.sleep(0.05)
        assert target is not None and owned(target), "Owned player window did not become viewable"
        x.XRaiseWindow(connection, target)
        x.XSetInputFocus(connection, target, 2, 0)
        x.XSync(connection, 0)
        focus, revert = window(), c.c_int()
        x.XGetInputFocus(connection, c.byref(focus), c.byref(revert))
        assert focus.value == target, "Focus did not reach the owned player"
        if action == "escape":
            key = x.XKeysymToKeycode(connection, x.XStringToKeysym(b"Escape"))
            assert key
            assert xt.XTestFakeKeyEvent(connection, key, 1, 0)
            x.XSync(connection, 0)
            time.sleep(0.08)
            assert xt.XTestFakeKeyEvent(connection, key, 0, 0)
        else:
            assert x.XGetWindowAttributes(connection, target, c.byref(attributes)) and attributes.map_state == 2
            assert attributes.width > 64 and attributes.height > 64
            px, py, child = c.c_int(), c.c_int(), window()
            assert x.XTranslateCoordinates(connection, target, root, 0, 0, c.byref(px), c.byref(py), c.byref(child))
            # Installed mpv 0.37's fixed bottombar Play/Pause and Fullscreen.
            local_x = attributes.width - 14 if action == "fullscreen" else 14
            assert xt.XTestFakeMotionEvent(connection, -1, px.value + local_x, py.value + attributes.height - 10, 0)
            x.XSync(connection, 0)
            time.sleep(0.35)  # Hover and separate deliberate clicks from double-clicks.
            assert owned(target)
            assert xt.XTestFakeButtonEvent(connection, 1, 1, 0)
            x.XSync(connection, 0)
            time.sleep(0.08)
            assert xt.XTestFakeButtonEvent(connection, 1, 0, 0)
        x.XSync(connection, 0)
        time.sleep(0.12)
        print(json.dumps({"ok": True, "isolated_xvfb": True, "real_x11_xtest_events": True, "owned_player_pid": pid, "action": action}))
    finally:
        x.XCloseDisplay(connection)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pid", type=int, required=True)
    parser.add_argument("--action", choices=("play-pause", "fullscreen", "escape"), required=True)
    args = parser.parse_args()
    act(args.pid, args.action)
