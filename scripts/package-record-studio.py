#!/usr/bin/env python3
"""Package/install an already-built Storyfold Record binary; never downloads tools."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parent.parent
ASSETS = ROOT / 'packaging' / 'record-studio'
DOWNLOADER = ROOT / 'data/record-studio-tools/yt-dlp'
DOWNLOADER_SHA256 = '1fa6733c37ea6fb51c99ad8fe785e7b7e5f3246c9b980230329d4fb72ed8d4d6'
DOWNLOADER_NOTICES = ('yt-dlp.LICENSE', 'yt-dlp.THIRD_PARTY_LICENSES')


def regular(path, *, allow_hardlinks=False):
    info = path.lstat()
    if not stat.S_ISREG(info.st_mode) or (info.st_nlink != 1 and not allow_hardlinks):
        raise ValueError(f'Expected an unlinked regular file: {path.name}')
    return info


def copy(source, destination, mode=0o644):
    regular(source, allow_hardlinks=True)  # Cargo hard-links its own output; we only read it.
    if any(folder.is_symlink() for folder in destination.parents):
        raise ValueError('Destination directory must not be a link')
    destination.parent.mkdir(parents=True, exist_ok=True)
    if destination.exists() or destination.is_symlink():
        regular(destination)
    with tempfile.NamedTemporaryFile(dir=destination.parent, delete=False) as temp:
        pending = Path(temp.name)
        try:
            with source.open('rb') as stream:
                shutil.copyfileobj(stream, temp)
            temp.flush()
            os.fsync(temp.fileno())
            os.fchmod(temp.fileno(), mode)
            os.replace(pending, destination)
        finally:
            pending.unlink(missing_ok=True)


def stage(binary, destination, downloader=None):
    copy(binary, destination / 'usr/bin/storyfold-record', 0o755)
    copy(ASSETS / 'storyfold-record.desktop', destination / 'usr/share/applications/storyfold-record.desktop')
    copy(ASSETS / 'storyfold-record.svg', destination / 'usr/share/icons/hicolor/scalable/apps/storyfold-record.svg')
    copy(ROOT / 'packages/video-core/assets/fonts/OFL.txt', destination / 'usr/share/doc/storyfold-record/Inter-OFL.txt')
    copy(ROOT / 'packages/video-core/assets/fonts/README.md', destination / 'usr/share/doc/storyfold-record/Inter-provenance.md')
    copy(ASSETS / 'README.md', destination / 'usr/share/doc/storyfold-record/README.md')
    if downloader is not None:
        copy(downloader, destination / 'usr/share/storyfold-record/tools/yt-dlp', 0o755)
        for name in DOWNLOADER_NOTICES:
            copy(downloader.parent / name, destination / 'usr/share/doc/storyfold-record' / name)
    for folder in destination.rglob('*'):
        if folder.is_dir():
            folder.chmod(0o755)


def deb(binary, output, version, downloader=None):
    output.mkdir(parents=True, exist_ok=True)
    architecture = subprocess.run(['dpkg', '--print-architecture'], capture_output=True, text=True, check=True).stdout.strip()
    if not re.fullmatch(r'[a-z0-9]+', architecture):
        raise ValueError('Unexpected Debian architecture')
    # The host is Ubuntu24.04/glibc2.39. Inspect actual ELF version requirements
    # rather than claiming compatibility with a release we have not tested.
    elf = subprocess.run(['readelf', '--version-info', str(binary)], capture_output=True, text=True, check=True).stdout
    required = max((tuple(map(int, value.split('.'))) for value in re.findall(r'GLIBC_([0-9.]+)', elf)), default=(2, 39))
    glibc = '.'.join(map(str, required))
    with tempfile.TemporaryDirectory(prefix='storyfold-package-') as temporary:
        target = Path(temporary) / 'storyfold-record'
        stage(binary, target, downloader)
        control = target / 'DEBIAN'
        control.mkdir()
        (control / 'control').write_text(f'''Package: storyfold-record
Version: {version}
Section: video
Priority: optional
Architecture: {architecture}
Maintainer: Storyfold maintainers
Depends: libc6 (>= {glibc}), libgcc-s1, libstdc++6, libx11-6, libxcursor1, libxrandr2, libxi6, libxkbcommon0, libxkbcommon-x11-0, libwayland-client0, libgl1, libegl1, libdbus-1-3, ffmpeg, mpv, tesseract-ocr, python3 (>= 3.10)
Recommends: chromium-browser | google-chrome-stable, gnome-keyring
Description: Storyfold native Record studio
 Continuous local recording, ordered takes, reviewed cards and sequence export.
 Camera and microphone start only after an explicit action. Media remains local.
''')
        artifact = output / f'storyfold-record_{version}_{architecture}.deb'
        if artifact.exists() or artifact.is_symlink():
            raise ValueError('Package output already exists; choose a new version or output folder')
        subprocess.run(['dpkg-deb', '--root-owner-group', '--build', str(target), str(artifact)], check=True)
    return artifact


def desktop_executable(path):
    # Desktop Exec quoting is not shell quoting; keep the installed path literal.
    return '"' + str(path).replace('\\', '\\\\').replace('"', '\\"').replace('`', '\\`').replace('$', '\\$') + '"'


def install(binary, downloader=None):
    prefix = Path.home() / '.local'
    installed = prefix / 'bin/storyfold-record'
    manifest = prefix / 'share/storyfold-record/install.json'
    with binary.open('rb') as stream:
        digest = hashlib.file_digest(stream, 'sha256').hexdigest()
    previous = {}
    if manifest.exists() or manifest.is_symlink():
        regular(manifest)
        previous = json.loads(manifest.read_text())
        if previous.get('application') != 'Storyfold Record Studio':
            raise ValueError('Unrecognized installation manifest')
    if installed.exists() and not manifest.is_file():
        regular(installed)
        with installed.open('rb') as stream:
            existing = hashlib.file_digest(stream, 'sha256').hexdigest()
        if existing != digest:
            raise ValueError('An unmanaged storyfold-record executable exists; refusing to replace it')
    downloader_digest = None
    if downloader is not None:
        regular(downloader)
        with downloader.open('rb') as stream:
            downloader_digest = hashlib.file_digest(stream, 'sha256').hexdigest()
        tool = manifest.parent / 'tools/yt-dlp'
        if tool.exists() or tool.is_symlink():
            regular(tool)
            with tool.open('rb') as stream:
                existing = hashlib.file_digest(stream, 'sha256').hexdigest()
            if existing != downloader_digest and previous.get('yt_dlp_sha256') != existing:
                raise ValueError('An unmanaged downloader exists; refusing to replace it')
        copy(downloader, tool, 0o755)
        for name in DOWNLOADER_NOTICES:
            copy(downloader.parent / name, manifest.parent / 'licenses' / name)
    icon = manifest.parent / 'icon.svg'
    copy(ASSETS / 'storyfold-record.svg', icon)
    copy(binary, installed, 0o755)
    launcher = prefix / 'share/applications/storyfold-record.desktop'
    copy(ASSETS / 'storyfold-record.desktop', launcher)
    launcher.write_text(launcher.read_text().replace('Exec=storyfold-record', 'Exec=' + desktop_executable(installed)).replace('Icon=storyfold-record', 'Icon=' + str(icon)))
    manifest.parent.mkdir(parents=True, exist_ok=True)
    data = {'application': 'Storyfold Record Studio', 'sha256': digest}
    if downloader_digest is not None:
        data['yt_dlp_sha256'] = downloader_digest
    # No source/project paths, credentials, recordings or model content enter the manifest.
    with tempfile.NamedTemporaryFile(mode='w', dir=manifest.parent, delete=False) as temp:
        pending = Path(temp.name)
        try:
            json.dump(data, temp, indent=2)
            temp.write('\n')
            temp.flush()
            os.fsync(temp.fileno())
            if manifest.exists() or manifest.is_symlink():
                regular(manifest)
            os.replace(pending, manifest)
        finally:
            pending.unlink(missing_ok=True)
    return installed


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, default=ROOT / 'target/release/sidebar-video')
    parser.add_argument('--output', type=Path, default=ROOT / 'dist')
    parser.add_argument('--version', default='0.1.1')
    parser.add_argument('--install-user', action='store_true')
    args = parser.parse_args()
    regular(args.binary, allow_hardlinks=True)
    binary = args.binary.resolve()
    with binary.open('rb') as stream:
        header = stream.read(4)
    if not binary.is_relative_to(ROOT) or header != b'\x7fELF':
        raise ValueError('Use the built Linux executable from this workspace')
    if not re.fullmatch(r'[0-9]+(?:[.+~-][A-Za-z0-9]+)*', args.version):
        raise ValueError('Invalid Debian version')
    regular(DOWNLOADER)
    with DOWNLOADER.open('rb') as stream:
        if hashlib.file_digest(stream, 'sha256').hexdigest() != DOWNLOADER_SHA256:
            raise ValueError('Bundled yt-dlp does not match the pinned official release checksum')
    for name in DOWNLOADER_NOTICES:
        regular(DOWNLOADER.parent / name)
    output = args.output.resolve()
    if not output.is_relative_to(ROOT):
        raise ValueError('Keep package artifacts within this workspace')
    artifact = install(binary, DOWNLOADER) if args.install_user else deb(binary, output, args.version, DOWNLOADER)
    print(artifact)


if __name__ == '__main__':
    main()
