#!/usr/bin/env python3
from __future__ import annotations

import hashlib
import re
import shutil
import urllib.request
import zipfile
from pathlib import Path


DENO_VERSION = "2.9.6"
DENO_URL = f"https://github.com/denoland/deno/releases/download/v{DENO_VERSION}/deno-x86_64-pc-windows-msvc.zip"
def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def main() -> int:
    project_dir = Path(__file__).resolve().parents[1]
    vendor_dir = project_dir / "vendor" / "deno"
    destination = vendor_dir / "deno.exe"
    if destination.exists():
        print(f"Deno already prepared: {destination}")
        return 0

    cache_dir = project_dir / ".cache" / "deno"
    archive = cache_dir / f"deno-{DENO_VERSION}.zip"
    checksum_file = cache_dir / "deno.sha256sum"
    cache_dir.mkdir(parents=True, exist_ok=True)
    vendor_dir.mkdir(parents=True, exist_ok=True)
    if not archive.exists():
        urllib.request.urlretrieve(DENO_URL, archive)
    checksum_url = f"{DENO_URL}.sha256sum"
    urllib.request.urlretrieve(checksum_url, checksum_file)
    checksum_text = checksum_file.read_text(encoding="utf-8-sig")
    matches = re.findall(r"(?i)\b[0-9a-f]{64}\b", checksum_text)
    if not matches:
        raise RuntimeError("Deno checksum file does not contain a SHA-256 value.")
    expected = matches[0].casefold()
    actual = sha256(archive)
    if expected != actual:
        archive.unlink(missing_ok=True)
        raise RuntimeError(f"Deno checksum mismatch: expected {expected}, got {actual}")
    with zipfile.ZipFile(archive) as bundle:
        with bundle.open("deno.exe") as source, destination.open("wb") as target:
            shutil.copyfileobj(source, target)
    print(f"Prepared bundled Deno: {destination}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
