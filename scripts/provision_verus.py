#!/usr/bin/env python3
"""Provision pinned Verus into a private home inside verify_local containment.

Invoke: python3 scripts/verify_local.py --seconds 600 -- python3 scripts/provision_verus.py
Requires an existing rustup launcher; never modifies its global toolchain home.
Existing installations are authenticated and reused, not repaired or replaced.
"""

import argparse
import hashlib
import os
from pathlib import Path, PurePosixPath
import shutil
import stat
import subprocess
import sys
import tempfile
import urllib.request
import zipfile

from kani_release import hash_file, load_json, require_directory, require_owned_path
from verify_local import check_limits
from verify_verus import ROOT, checked_toolchain, tool_home


def private_home(path):
    path = path.expanduser().absolute()
    # A sticky root-owned /tmp is allowed as an ancestor, never as the tool home.
    for ancestor in reversed(path.parents):
        if not ancestor.exists() and not ancestor.is_symlink():
            ancestor.mkdir(mode=0o700)
        metadata = ancestor.lstat()
        if not stat.S_ISDIR(metadata.st_mode) or ancestor.is_symlink():
            raise ValueError(f"unsafe tool ancestor: {ancestor}")
        if metadata.st_uid not in {0, os.geteuid()}:
            raise ValueError(f"untrusted tool ancestor owner: {ancestor}")
        if metadata.st_mode & (stat.S_IWGRP | stat.S_IWOTH):
            if not (metadata.st_uid == 0 and metadata.st_mode & stat.S_ISVTX):
                raise ValueError(f"writable tool ancestor: {ancestor}")
    path.mkdir(mode=0o700, exist_ok=True)
    require_directory(path.parent, "Verus tool parent", recursive=False)
    return require_directory(path, "Verus home")


def download(url, destination, expected):
    if not url.startswith("https://"):
        raise ValueError("pinned downloads require HTTPS")
    digest = hashlib.sha256()
    try:
        with urllib.request.urlopen(url, timeout=60) as response, destination.open("xb") as output:
            if not response.url.startswith("https://"):
                raise ValueError("download redirected away from HTTPS")
            while block := response.read(1024 * 1024):
                digest.update(block)
                output.write(block)
    except OSError as error:
        raise ValueError(f"pinned download failed: {url}: {error}") from error
    if digest.hexdigest() != expected or hash_file(destination) != expected:
        raise ValueError(f"pinned download hash mismatch: {url}")


def extract_bundle(archive, destination):
    """Reject path escapes, aliases, special files and unsafe archive modes."""
    with zipfile.ZipFile(archive) as source:
        names = set()
        for member in source.infolist():
            name = PurePosixPath(member.filename)
            mode = member.external_attr >> 16
            kind = stat.S_IFMT(mode)
            if (not name.parts or name.parts[0] != "verus-x86-linux"
                    or name.is_absolute() or ".." in name.parts
                    or "\\" in member.filename or name in names
                    or kind not in {0, stat.S_IFREG, stat.S_IFDIR}
                    or (member.is_dir() and kind == stat.S_IFREG)
                    or (not member.is_dir() and kind == stat.S_IFDIR)):
                raise ValueError(f"unsafe Verus archive member: {member.filename}")
            names.add(name)
            target = destination.joinpath(*name.parts)
            if member.is_dir():
                target.mkdir(mode=0o700, parents=True, exist_ok=True)
            else:
                target.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
                with source.open(member) as input_file, target.open("xb") as output:
                    shutil.copyfileobj(input_file, output, 1024 * 1024)
                target.chmod(0o700 if mode & 0o111 else 0o600)
    require_directory(destination / "verus-x86-linux", "extracted Verus bundle")


def provision(home, pin):
    check_limits()
    # The parent must exist so callers choose and retain custody of its location.
    home = private_home(home)
    bundle = home / "verus-x86-linux"
    toolchain = home / "rustup/toolchains" / pin["rust"]["toolchain"]
    if bundle.exists() and toolchain.exists():
        checked_toolchain(pin, home)
        print(f"Pinned Verus already installed: {home}")
        return
    launcher = shutil.which("rustup")
    if launcher is None:
        raise ValueError("an existing rustup launcher is required")
    env = {**os.environ, "RUSTUP_HOME": str(home / "rustup"),
           "CARGO_HOME": str(home / "cargo"), "RUSTUP_AUTO_INSTALL": "0",
           "RUSTUP_DIST_SERVER": "https://static.rust-lang.org",
           "RUSTUP_UPDATE_ROOT": "https://static.rust-lang.org/rustup",
           "RUSTUP_IO_THREADS": "1", "RUSTUP_CONCURRENT_DOWNLOADS": "1",
           "RUSTUP_UNPACK_RAM": str(256 * 1024**2)}
    for directory in (home / "rustup", home / "cargo"):
        directory.mkdir(mode=0o700, exist_ok=True)
        require_directory(directory, "isolated Rust home")
    with tempfile.TemporaryDirectory(prefix=".provision-", dir=home) as temporary:
        stage = Path(temporary)
        if not bundle.exists():
            archive = stage / "verus.zip"
            download(pin["verus"]["download_url"], archive, pin["verus"]["archive_sha256"])
            extract_bundle(archive, stage)
            (stage / "verus-x86-linux").rename(bundle)
        if not toolchain.exists():
            # Authenticate immutable release metadata before rustup consumes it;
            # rustup checks each component against this release's own digests.
            download(pin["rust"]["release_manifest_url"], stage / "rust-manifest.toml",
                     pin["rust"]["published_release_manifest_sha256"])
            subprocess.run([launcher, "set", "auto-self-update", "disable"],
                           env=env, check=True, timeout=30)
            command = [launcher, "toolchain", "install", pin["rust"]["toolchain"],
                       "--profile", "minimal", "--no-self-update"]
            for component in pin["rust"]["installed_components"]:
                command.extend(["--component", component])
            subprocess.run(command, env=env, check=True, timeout=540)
    require_owned_path(home, "Verus home")
    checked_toolchain(pin, home)
    print(f"Pinned Verus provisioned: {home}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--home", type=Path)
    args = parser.parse_args()
    pin = load_json(ROOT / "verification/verus/toolchain.json")
    provision(args.home if args.home is not None else tool_home(pin), pin)
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError, subprocess.SubprocessError, zipfile.BadZipFile) as error:
        print(f"provision-verus: {error}", file=sys.stderr)
        sys.exit(1)
