import argparse
import html
import json
import os
import re
import sys
import urllib.request
from dataclasses import dataclass
from datetime import datetime
from typing import Dict, List, Optional, Tuple

import yt_dlp


@dataclass
class CaptionTrack:
    lang: str
    ext: str
    url: str
    is_auto: bool


_TIME_RE_VTT = re.compile(
    r"^(?P<start>\d{2}:\d{2}:\d{2}\.\d{3})\s*-->\s*(?P<end>\d{2}:\d{2}:\d{2}\.\d{3})"
)

_TAG_RE = re.compile(r"<[^>]+>")


def _fetch_bytes(url: str, timeout: int = 30) -> bytes:
    req = urllib.request.Request(
        url,
        headers={
            "User-Agent": "Mozilla/5.0 (macOS) Python-script subtitle fetch"
        },
    )
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        return resp.read()


def _normalize_ts(ts: str) -> str:
    # "HH:MM:SS.mmm" -> "HH:MM:SS"
    return ts.split(".")[0]


def _clean_caption_text(s: str) -> str:
    s = html.unescape(s)
    s = _TAG_RE.sub("", s)
    s = s.replace("\u00a0", " ")
    s = re.sub(r"\s+", " ", s).strip()
    return s


def _pick_lang(available_langs: List[str], lang_regex: str) -> Optional[str]:
    pat = re.compile(lang_regex)
    # Deterministisch: zuerst direkte Treffer in sortierter Reihenfolge,
    # danach bleibt None, falls kein Treffer existiert.
    for lang in sorted(available_langs):
        if pat.fullmatch(lang) or pat.match(lang):
            return lang
    return None


def _select_track(info: dict, lang_regex: str, prefer_official: bool) -> Optional[CaptionTrack]:
    subs: Dict[str, List[dict]] = info.get("subtitles") or {}
    autos: Dict[str, List[dict]] = info.get("automatic_captions") or {}

    candidate_langs = sorted(set(list(subs.keys()) + list(autos.keys())))
    chosen_lang = _pick_lang(candidate_langs, lang_regex)
    if chosen_lang is None:
        return None

    tracks_official = subs.get(chosen_lang) or []
    tracks_auto = autos.get(chosen_lang) or []

    def best_format(track_list: List[dict], is_auto: bool) -> Optional[CaptionTrack]:
        # Prioritaet: vtt, json3, ttml, srv3, srv2, srv1
        pref = ["vtt", "json3", "ttml", "srv3", "srv2", "srv1"]
        by_ext: Dict[str, dict] = {}
        for t in track_list:
            ext = (t.get("ext") or "").lower()
            url = t.get("url")
            if ext and url and ext not in by_ext:
                by_ext[ext] = t
        for ext in pref:
            if ext in by_ext:
                return CaptionTrack(lang=chosen_lang, ext=ext, url=by_ext[ext]["url"], is_auto=is_auto)
        # Fallback: irgendein erstes Element mit url
        for t in track_list:
            ext = (t.get("ext") or "").lower()
            url = t.get("url")
            if url:
                return CaptionTrack(lang=chosen_lang, ext=ext or "unknown", url=url, is_auto=is_auto)
        return None

    if prefer_official and tracks_official:
        picked = best_format(tracks_official, is_auto=False)
        if picked is not None:
            return picked

    if tracks_official:
        picked = best_format(tracks_official, is_auto=False)
        if picked is not None:
            return picked

    if tracks_auto:
        picked = best_format(tracks_auto, is_auto=True)
        if picked is not None:
            return picked

    return None


def _parse_vtt(raw: str) -> List[Tuple[str, str]]:
    lines = raw.splitlines()
    out: List[Tuple[str, str]] = []
    i = 0

    while i < len(lines):
        line = lines[i].strip("\ufeff").strip()

        if not line:
            i += 1
            continue

        if line.startswith("WEBVTT") or line.startswith("NOTE") or line.startswith("STYLE") or line.startswith("REGION"):
            i += 1
            continue

        # Optional cue identifier line
        if i + 1 < len(lines) and _TIME_RE_VTT.match(lines[i + 1].strip()):
            i += 1
            line = lines[i].strip()

        m = _TIME_RE_VTT.match(line)
        if not m:
            i += 1
            continue

        start = _normalize_ts(m.group("start"))
        i += 1
        text_lines = []
        while i < len(lines) and lines[i].strip():
            text_lines.append(lines[i].strip())
            i += 1

        text = _clean_caption_text(" ".join(text_lines))
        if text:
            out.append((start, text))

    # einfache Deduplikation direkt aufeinanderfolgender identischer Texte
    dedup: List[Tuple[str, str]] = []
    last_text = None
    for ts, txt in out:
        if txt != last_text:
            dedup.append((ts, txt))
        last_text = txt
    return dedup


def _parse_json3(raw: str) -> List[Tuple[str, str]]:
    # JSON3: "events" mit "tStartMs" und "segs" (utf8)
    data = json.loads(raw)
    events = data.get("events") or []
    out: List[Tuple[str, str]] = []

    for ev in events:
        t_ms = ev.get("tStartMs")
        segs = ev.get("segs") or []
        if t_ms is None or not segs:
            continue
        txt = "".join(seg.get("utf8", "") for seg in segs)
        txt = _clean_caption_text(txt)
        if not txt:
            continue
        total_seconds = int(round(t_ms / 1000.0))
        hh = total_seconds // 3600
        mm = (total_seconds % 3600) // 60
        ss = total_seconds % 60
        ts = f"{hh:02d}:{mm:02d}:{ss:02d}"
        out.append((ts, txt))

    # Deduplikation direkt aufeinanderfolgender identischer Texte
    dedup: List[Tuple[str, str]] = []
    last_text = None
    for ts, txt in out:
        if txt != last_text:
            dedup.append((ts, txt))
        last_text = txt
    return dedup


def _to_markdown(
    title: str,
    url: str,
    track: CaptionTrack,
    items: List[Tuple[str, str]],
) -> str:
    now = datetime.utcnow().strftime("%Y-%m-%d %H:%M:%S UTC")
    source = "auto" if track.is_auto else "official"

    header = []
    header.append(f"# Transkript mit Timecodes")
    header.append("")
    header.append(f"Titel: {title}")
    header.append(f"URL: {url}")
    header.append(f"Sprache: {track.lang}")
    header.append(f"Quelle: {source}")
    header.append(f"Format: {track.ext}")
    header.append(f"Erstellt: {now}")
    header.append("")
    header.append("## Inhalt")
    header.append("")

    body = [f"- {ts} {txt}" for ts, txt in items]
    return "\n".join(header + body) + "\n"


def main() -> int:
    ap = argparse.ArgumentParser(
        prog="yt_transcript_md",
        description="Extrahiert YouTube-Untertitel und erzeugt ein Markdown-Transkript mit Timecodes.",
    )
    ap.add_argument("url", help="YouTube-URL")
    ap.add_argument(
        "--lang-regex",
        default="de|de-.*|en|en-.*",
        help='Regex fuer Sprachtags, z.B. "de|de-.*|en|en-.*"',
    )
    ap.add_argument(
        "--prefer-official",
        action="store_true",
        help="Creator-provided Untertitel werden bevorzugt, falls vorhanden.",
    )
    ap.add_argument(
        "--out",
        default=None,
        help="Ausgabepfad fuer Markdown, Default: ./<video_id>_<lang>.md",
    )
    args = ap.parse_args()

    ydl_opts = {
        "quiet": True,
        "skip_download": True,
        "noplaylist": True,
    }

    try:
        with yt_dlp.YoutubeDL(ydl_opts) as ydl:
            info = ydl.extract_info(args.url, download=False)
    except Exception as e:
        print(f"Fehler bei Metadatenextraktion: {e}", file=sys.stderr)
        return 2

    track = _select_track(info, args.lang_regex, args.prefer_official)
    if track is None:
        print("Keine passende Untertitelspur gefunden.", file=sys.stderr)
        return 3

    try:
        raw_bytes = _fetch_bytes(track.url)
    except Exception as e:
        print(f"Fehler beim Download der Untertitel: {e}", file=sys.stderr)
        return 4

    raw = raw_bytes.decode("utf-8", errors="replace")

    if track.ext == "vtt":
        items = _parse_vtt(raw)
    elif track.ext == "json3":
        items = _parse_json3(raw)
    else:
        # Fallback: Versuch als VTT; bei Fehlschlag leere Liste
        items = _parse_vtt(raw)

    if not items:
        print("Untertiteldatei konnte nicht in verwertbare Timecodes zerlegt werden.", file=sys.stderr)
        return 5

    title = (info.get("title") or "").strip() or "Unbekannter Titel"
    video_id = (info.get("id") or "video").strip()
    out_path = args.out
    if out_path is None:
        safe_lang = re.sub(r"[^A-Za-z0-9_.-]+", "_", track.lang)
        out_path = os.path.join(os.getcwd(), f"{video_id}_{safe_lang}.md")

    md = _to_markdown(title=title, url=args.url, track=track, items=items)

    try:
        with open(out_path, "w", encoding="utf-8") as f:
            f.write(md)
    except Exception as e:
        print(f"Fehler beim Schreiben der Datei: {e}", file=sys.stderr)
        return 6

    print(out_path)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
