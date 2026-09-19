#!/usr/bin/env python3
from __future__ import annotations

import shutil
import sys
import urllib.request
import zipfile
from pathlib import Path


FFMPEG_URL = "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip"


def find_tool(root: Path, name: str) -> Path:
    matches = sorted(root.glob(f"**/bin/{name}.exe"))
    if not matches:
        raise FileNotFoundError(f"{name}.exe was not found in the downloaded archive.")
    return matches[0]


def main() -> int:
    project_dir = Path(__file__).resolve().parents[1]
    vendor_dir = project_dir / "vendor" / "ffmpeg"
    tool_names = ("ffmpeg", "ffprobe", "ffplay")
    cache_dir = project_dir / ".cache" / "ffmpeg"
    archive_path = cache_dir / "ffmpeg-release-essentials.zip"
    extract_dir = cache_dir / "extract"

    if all((vendor_dir / f"{name}.exe").exists() for name in tool_names):
        print(f"FFmpeg tools already prepared: {vendor_dir}")
        return 0

    cache_dir.mkdir(parents=True, exist_ok=True)
    vendor_dir.mkdir(parents=True, exist_ok=True)

    if not archive_path.exists():
        print(f"Downloading FFmpeg from {FFMPEG_URL}")
        urllib.request.urlretrieve(FFMPEG_URL, archive_path)

    if extract_dir.exists():
        shutil.rmtree(extract_dir)
    extract_dir.mkdir(parents=True)

    print("Extracting FFmpeg ...")
    with zipfile.ZipFile(archive_path) as archive:
        archive.extractall(extract_dir)

    for name in tool_names:
        destination = vendor_dir / f"{name}.exe"
        shutil.copy2(find_tool(extract_dir, name), destination)
        print(f"Prepared bundled tool: {destination}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
