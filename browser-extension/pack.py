#!/usr/bin/env python3
"""Pack the add-on into an .xpi, the way Firefox installs one.

An .xpi is a zip, and the zip has to hold the manifest at the top level --
the same reason `about:debugging` wants `manifest.json` and not a directory
around it. Getting this wrong is silent: geckodriver answers `500` and says
nothing about why, which is an hour of wondering whether the add-on is
broken.

    python3 browser-extension/pack.py            # writes dist/…xpi
    python3 browser-extension/pack.py --print    # and says where

One script for both callers, so the file the user installs by hand and the
file the live check installs are the same file.
"""

import os
import sys
import zipfile

HERE = os.path.dirname(os.path.abspath(__file__))
# what an .xpi has to have.
EXTENSION = HERE
DIST = os.path.join(EXTENSION, "dist")

# Not in the .xpi: the tests need node or a browser and no add-on needs
# either, and the README is for a person reading the repository rather than
# for Firefox reading a directory.
SKIP_DIRS = {"test", "node_modules", "dist", "__pycache__"}
SKIP_SUFFIXES = (".md", ".mjs", ".py", ".pyc")
SKIP_NAMES = {".DS_Store"}


def version():
    """The manifest's own version, read with a line scan.

    Not a json import: the manifest is JSON-with-comments territory by
    habit and the only field needed here is a string, and a line that starts
    with `"version"` is not a parser either way.
    """
    with open(os.path.join(EXTENSION, "manifest.json"), encoding="utf-8") as f:
        for line in f:
            stripped = line.strip().rstrip(",")
            if stripped.startswith('"version"'):
                return stripped.split(":", 1)[1].strip().strip('"')
    raise SystemExit("manifest.json has no version")


def pack(destination=None):
    name = f"doris-search-bridge-{version()}.xpi"
    os.makedirs(DIST, exist_ok=True)
    target = destination or os.path.join(DIST, name)

    written = []
    with zipfile.ZipFile(target, "w", zipfile.ZIP_DEFLATED) as out:
        for root, dirs, files in os.walk(EXTENSION):
            dirs[:] = [d for d in dirs if d not in SKIP_DIRS]
            for entry in sorted(files):
                if entry.endswith(SKIP_SUFFIXES) or entry in SKIP_NAMES:
                    continue
                path = os.path.join(root, entry)
                # arcname relative to the extension root: the manifest has
                # to be at the top of the zip for Firefox to find it.
                out.write(path, os.path.relpath(path, EXTENSION))
                written.append(os.path.relpath(path, EXTENSION))

    if "manifest.json" not in written:
        raise SystemExit(f"{target} has no manifest.json at the top; it will not load")
    return target, written


if __name__ == "__main__":
    path, files = pack()
    size = os.path.getsize(path)
    print(f"{path}\n  {size} bytes, {len(files)} files")
    for name in files:
        print(f"  {name}")
    if "--print" not in sys.argv:
        print(f"\ninstall it: about:addons -> the gear -> Install Add-on From File")