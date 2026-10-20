"""Own-app OS event fixture. Requires a fresh xvfb-run display, never the desktop."""
import ctypes as c
import ctypes.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time

assert os.environ.get('STORYFOLD_INPUT_FIXTURE') == '1'
assert 'xvfb-run.' in os.environ.get('XAUTHORITY', ''), 'Use a new isolated Xvfb display'
root = Path(__file__).resolve().parent.parent
x = c.CDLL(ctypes.util.find_library('X11'))
t = c.CDLL(ctypes.util.find_library('Xtst'))
display = c.c_void_p
window = c.c_ulong
class Attributes(c.Structure):
    _fields_ = [(name, c.c_int) for name in ('x','y','width','height','border_width','depth')] + [
        ('visual',c.c_void_p),('root',window),('window_class',c.c_int),('bit_gravity',c.c_int),
        ('win_gravity',c.c_int),('backing_store',c.c_int),('backing_planes',c.c_ulong),
        ('backing_pixel',c.c_ulong),('save_under',c.c_int),('colormap',c.c_ulong),
        ('map_installed',c.c_int),('map_state',c.c_int),('all_event_masks',c.c_long),
        ('your_event_mask',c.c_long),('do_not_propagate_mask',c.c_long),
        ('override_redirect',c.c_int),('screen',c.c_void_p)]
x.XOpenDisplay.argtypes = [c.c_char_p]
x.XOpenDisplay.restype = display
x.XDefaultRootWindow.argtypes = [display]
x.XDefaultRootWindow.restype = window
x.XQueryTree.argtypes = [display, window, c.POINTER(window), c.POINTER(window), c.POINTER(c.POINTER(window)), c.POINTER(c.c_uint)]
x.XFetchName.argtypes = [display, window, c.POINTER(c.c_void_p)]
x.XFree.argtypes = [c.c_void_p]
x.XRaiseWindow.argtypes = [display, window]
x.XSetInputFocus.argtypes = [display, window, c.c_int, c.c_ulong]
x.XTranslateCoordinates.argtypes = [display, window, window, c.c_int, c.c_int, c.POINTER(c.c_int), c.POINTER(c.c_int), c.POINTER(window)]
x.XFlush.argtypes = [display]
x.XCloseDisplay.argtypes = [display]
x.XGetWindowAttributes.argtypes = [display, window, c.POINTER(Attributes)]
t.XTestFakeMotionEvent.argtypes = [display, c.c_int, c.c_int, c.c_int, c.c_ulong]
t.XTestFakeButtonEvent.argtypes = [display, c.c_uint, c.c_int, c.c_ulong]

def records(path):
    rows = []
    for line in path.read_text().splitlines():
        try:
            rows.append(json.loads(line))
        except json.JSONDecodeError:
            pass
    return rows

with tempfile.TemporaryDirectory(prefix='sf-native-input-') as temporary:
    log = Path(temporary) / 'probe.log'
    env = os.environ.copy()
    env['STORYFOLD_RECORD_INPUT_PROBE'] = '1'
    env.pop('EFRAME_SCREENSHOT_TO', None)
    env.pop('WAYLAND_DISPLAY', None)
    env.pop('WAYLAND_SOCKET', None)
    with log.open('w') as stream:
        app = subprocess.Popen([str(root / 'target/debug/sidebar-video')], env=env, stdin=subprocess.DEVNULL, stdout=stream, stderr=stream)
    connection = None
    try:
        connection = x.XOpenDisplay(None)
        assert connection
        desktop = x.XDefaultRootWindow(connection)
        deadline = time.monotonic() + 15
        owned = None
        while time.monotonic() < deadline:
            assert app.poll() is None, log.read_text()[-3000:]
            children = c.POINTER(window)()
            count = c.c_uint()
            a, b = window(), window()
            x.XQueryTree(connection, desktop, c.byref(a), c.byref(b), c.byref(children), c.byref(count))
            for index in range(count.value):
                name = c.c_void_p()
                if x.XFetchName(connection, children[index], c.byref(name)) and name.value:
                    title = c.string_at(name).decode(errors='replace')
                    x.XFree(name)
                    if 'Storyfold' in title and 'Input check' in title:
                        owned = int(children[index])
                        break
            if children:
                x.XFree(children)
            if owned:
                break
            time.sleep(.1)
        assert owned, log.read_text()[-3000:]
        attributes = Attributes()
        while True:
            assert x.XGetWindowAttributes(connection, owned, c.byref(attributes))
            if attributes.map_state == 2:
                break
            assert time.monotonic() < deadline, 'Owned fixture window did not become viewable'
            time.sleep(.05)
        x.XRaiseWindow(connection, owned)
        x.XSetInputFocus(connection, owned, 2, 0)
        x.XFlush(connection)
        time.sleep(.6)
        px, py, child = c.c_int(), c.c_int(), window()
        assert x.XTranslateCoordinates(connection, owned, desktop, 0, 0, c.byref(px), c.byref(py), c.byref(child))
        menu = next(row['project_menu'] for row in reversed(records(log)) if row.get('project_menu'))
        latest = records(log)[-1]
        scale = latest.get('pixels_per_point', 1)
        bounds = menu['rect']
        assert menu['enabled']
        t.XTestFakeMotionEvent(connection, -1, px.value + int((bounds[0]+bounds[2])*scale/2), py.value + int((bounds[1]+bounds[3])*scale/2), 0)
        x.XFlush(connection)
        time.sleep(.15)
        t.XTestFakeButtonEvent(connection, 1, 1, 0)
        x.XFlush(connection)
        time.sleep(.15)
        t.XTestFakeButtonEvent(connection, 1, 0, 0)
        x.XFlush(connection)
        time.sleep(.6)
        rows = records(log)
        assert any(row.get('pointer_buttons', 0) for row in rows), log.read_text()[-5000:]
        assert any(row.get('project_menu', {}).get('popup_open') for row in rows if row.get('project_menu')), log.read_text()[-5000:]
        result = {'isolated_xvfb': True, 'real_x11_xtest_events': True, 'native_pointer_button_received': True, 'project_menu_opens': True, 'probe_records': rows}
        output = root / 'data/record-studio-input'
        output.mkdir(parents=True, exist_ok=True)
        (output / 'native-os-input.json').write_text(json.dumps(result, indent=2) + '\n')
        print(json.dumps({k:v for k,v in result.items() if k!='probe_records'}))
        print(json.dumps({'records':len(rows),'last':rows[-1]},indent=2))
    finally:
        if app.poll() is None:
            app.terminate()
        app.wait(timeout=10)
        if connection:
            x.XCloseDisplay(connection)
