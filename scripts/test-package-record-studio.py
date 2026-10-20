#!/usr/bin/env python3
"""Isolated install checks; no real home directory or built app is needed."""
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('packager', Path(__file__).with_name('package-record-studio.py'))
packager = importlib.util.module_from_spec(spec)
spec.loader.exec_module(packager)

with tempfile.TemporaryDirectory(prefix='storyfold install test ') as temporary:
    root = Path(temporary)
    binary = root / 'generated-binary'
    binary.write_bytes(b'generated executable fixture')
    home = root / 'home'
    with patch.object(Path, 'home', return_value=home):
        installed = packager.install(binary)
        assert installed.read_bytes() == binary.read_bytes()
        assert installed.stat().st_mode & 0o777 == 0o755
        manifest = json.loads((home / '.local/share/storyfold-record/install.json').read_text())
        assert manifest == {'application': 'Storyfold Record Studio', 'sha256': hashlib.sha256(binary.read_bytes()).hexdigest()}
        launcher = home / '.local/share/applications/storyfold-record.desktop'
        assert 'Exec=' + packager.desktop_executable(installed) in launcher.read_text()
        subprocess.run(['desktop-file-validate', str(launcher)], check=True)
        packager.install(binary)  # Managed reinstall succeeds.
        (home / '.local/share/storyfold-record/install.json').unlink()
        packager.install(binary)  # Recover an interrupted installation of these exact bytes.
        (home / '.local/share/storyfold-record/install.json').unlink()
        installed.write_bytes(b'Unmanaged different executable')
        try:
            packager.install(binary)
            raise AssertionError('Unmanaged executable was overwritten')
        except ValueError:
            pass
    linked = root / 'linked'
    linked.symlink_to(binary)
    try:
        packager.copy(binary, linked)
        raise AssertionError('Linked destination was overwritten')
    except ValueError:
        pass
    assert binary.read_bytes() == b'generated executable fixture'
print('Isolated package installer checks passed')
