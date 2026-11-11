#!/usr/bin/env python3
"""Append licenses for newly locked macOS crates, preserving collected upstream texts.
Development tool only; not distributed as an application runtime.
"""
import gzip
import html
import json
from pathlib import Path
import re
import subprocess

root = Path(__file__).resolve().parent.parent
path = root / "THIRD-PARTY-LICENSES.html.gz"
existing = gzip.decompress(path.read_bytes()).decode()
metadata = json.loads(subprocess.check_output(
    ["cargo", "metadata", "--locked", "--format-version", "1", "--filter-platform", "aarch64-apple-darwin"], cwd=root))
resolved = {node["id"] for node in metadata["resolve"]["nodes"]}
additions = []
missing = []
for package in sorted(metadata["packages"], key=lambda p: (p["name"], p["version"])):
    if package["id"] not in resolved or package["name"] == "transcription-rust":
        continue
    title = f'{package["name"]} {package["version"]}'
    if f"<h2>{html.escape(title)}</h2>" in existing:
        continue
    folder = Path(package["manifest_path"]).parent
    licenses = [p for p in folder.iterdir() if p.is_file() and re.match(r"(license|licence|copying|notice)", p.name, re.I)]
    if package.get("license_file"):
        specified = folder / package["license_file"]
        if specified.is_file() and specified not in licenses:
            licenses.append(specified)
    if not licenses:
        supplemental=root / "licenses" / f'{package["name"]}-{package["version"]}'
        if supplemental.is_dir():
            licenses=[p for p in supplemental.iterdir() if p.name.startswith("LICENSE")]
    if not licenses:
        missing.append(title)
    section = f'<h2>{html.escape(title)}</h2><p>{html.escape(package.get("license") or "See upstream")}</p>'
    for license_path in sorted(licenses):
        section += f'<h3>{html.escape(license_path.name)}</h3><pre>{html.escape(license_path.read_text(errors="replace"))}</pre>'
    if package.get("repository"):
        url = html.escape(package["repository"], quote=True)
        section += f'<p><a href="{url}">Corresponding upstream source</a></p>'
    additions.append(section)
if missing:
    raise SystemExit("Missing packaged license texts; collect upstream notices first: " + ", ".join(missing))
if additions:
    path.write_bytes(gzip.compress((existing + "\n" + "\n".join(additions)).encode(), mtime=0))
print(f"Added {len(additions)} locked crate notices; retained existing upstream texts.")
