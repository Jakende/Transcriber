from __future__ import annotations

import json
import re
import subprocess
from pathlib import Path
from typing import Callable

from .models import WordToken


ProgressCallback = Callable[[int, str], None]


def normalize_audio(source: Path, destination: Path, ffmpeg: Path) -> None:
    ensure_audio_stream(source, ffmpeg)
    command = [
        str(ffmpeg), "-hide_banner", "-loglevel", "error", "-y",
        "-i", str(source), "-map", "0:a:0", "-vn", "-sn", "-dn",
        "-ar", "16000", "-ac", "1", "-c:a", "pcm_s16le",
        str(destination),
    ]
    result = subprocess.run(command, capture_output=True, text=True)
    if result.returncode != 0:
        detail = (result.stderr or result.stdout or "Unbekannter ffmpeg-Fehler").strip()
        raise RuntimeError(normalization_error(detail))


def ensure_audio_stream(source: Path, ffmpeg: Path) -> None:
    ffprobe = ffmpeg.with_name("ffprobe")
    if not ffprobe.exists():
        return
    result = subprocess.run(
        [
            str(ffprobe), "-v", "error", "-select_streams", "a",
            "-show_entries", "stream=index", "-of", "csv=p=0", str(source),
        ],
        capture_output=True,
        text=True,
    )
    if result.returncode == 0 and not result.stdout.strip():
        raise RuntimeError("Die Datei enthält keine Audiospur, die transkribiert werden kann.")


def normalization_error(detail: str) -> str:
    lowered = detail.casefold()
    if "matches no streams" in lowered or "does not contain any stream" in lowered:
        return "Die Datei enthält keine Audiospur, die transkribiert werden kann."
    return f"Audio- oder Video-Konvertierung fehlgeschlagen: {detail[-1200:]}"


def transcribe_words(
    wav_path: Path,
    whisper_cli: Path,
    model_path: Path,
    language: str,
    output_prefix: Path,
    progress: ProgressCallback,
    prompt_text: str | None = None,
) -> list[WordToken]:
    command = [
        str(whisper_cli), "-m", str(model_path), "-l", language,
        "-f", str(wav_path), "-ojf", "-sow", "-of", str(output_prefix),
        "--print-progress",
    ]
    if prompt_text:
        command.extend(["--prompt", prompt_text, "--carry-initial-prompt"])
    process = subprocess.Popen(
        command,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        bufsize=1,
    )
    assert process.stdout is not None
    for line in process.stdout:
        match = re.search(r"progress\s*=\s*(\d+)%", line)
        if match:
            progress(max(0, min(100, int(match.group(1)))), "Transkribiere …")
    return_code = process.wait()
    if return_code != 0:
        raise RuntimeError(f"whisper-cli wurde mit Code {return_code} beendet.")

    json_path = Path(str(output_prefix) + ".json")
    if not json_path.exists():
        raise RuntimeError(f"Whisper-Ausgabe fehlt: {json_path}")
    try:
        return parse_whisper_json(json_path)
    finally:
        json_path.unlink(missing_ok=True)


def parse_whisper_json(path: Path) -> list[WordToken]:
    data = json.loads(path.read_text(encoding="utf-8"))
    tokens: list[WordToken] = []
    for item in data.get("transcription", []):
        detailed = item.get("tokens") or []
        if detailed:
            tokens.extend(coalesce_word_tokens(detailed))
            continue
        text = str(item.get("text") or "")
        timestamps = item.get("timestamps") or {}
        if not text or "from" not in timestamps or "to" not in timestamps:
            continue
        tokens.append(
            WordToken(
                start=parse_timestamp(str(timestamps["from"])),
                end=parse_timestamp(str(timestamps["to"])),
                text=text,
            )
        )
    return tokens


def coalesce_word_tokens(items: list[dict]) -> list[WordToken]:
    """Join Whisper subword tokens so diarization never cuts through a word."""
    words: list[WordToken] = []
    current: WordToken | None = None
    for item in items:
        text = str(item.get("text") or "")
        timestamps = item.get("timestamps") or {}
        if not text or (text.startswith("[_") and text.endswith("]")):
            continue
        if "from" not in timestamps or "to" not in timestamps:
            continue

        start = parse_timestamp(str(timestamps["from"]))
        end = parse_timestamp(str(timestamps["to"]))
        punctuation_only = not any(character.isalnum() for character in text)
        starts_word = text[:1].isspace()

        if punctuation_only and current is not None:
            current.text += text
            continue
        if current is None or starts_word:
            if current is not None:
                words.append(current)
            current = WordToken(start=start, end=end, text=text)
        else:
            current.text += text
            current.end = max(current.end, end)

    if current is not None:
        words.append(current)
    return words


def parse_timestamp(value: str) -> float:
    parts = value.replace(",", ".").split(":")
    if len(parts) != 3:
        return 0.0
    return int(parts[0]) * 3600 + int(parts[1]) * 60 + float(parts[2])


def media_duration(path: Path, ffmpeg: Path) -> float | None:
    ffprobe = ffmpeg.with_name("ffprobe")
    if not ffprobe.exists():
        return None
    result = subprocess.run(
        [str(ffprobe), "-v", "error", "-show_entries", "format=duration", "-of", "default=nw=1:nk=1", str(path)],
        capture_output=True,
        text=True,
    )
    try:
        return float(result.stdout.strip()) if result.returncode == 0 else None
    except ValueError:
        return None
