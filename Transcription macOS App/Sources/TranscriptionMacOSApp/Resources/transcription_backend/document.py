from __future__ import annotations

import csv
import html
import json
import os
import re
import tempfile
import uuid
from datetime import datetime
from io import StringIO
from pathlib import Path

from .models import SpeakerRegion, TranscriptDocument, TranscriptSegment, WordToken


FORMATS = {"markdown": ".md", "vtt": ".vtt", "srt": ".srt", "txt": ".txt", "csv": ".csv"}


def build_document(
    source: Path,
    language: str,
    model: str,
    tokens: list[WordToken],
    regions: list[SpeakerRegion],
    include_timecodes: bool,
    podcast: dict | None = None,
) -> TranscriptDocument:
    speakers = sorted({region.speaker for region in regions})
    names = {speaker: f"Sprecher {index + 1}" for index, speaker in enumerate(speakers)}
    return TranscriptDocument(
        id=str(uuid.uuid4()),
        source_path=str(source),
        source_file=source.name,
        language=language,
        model=model,
        created=datetime.now().strftime("%Y-%m-%d %H:%M"),
        timecodes=include_timecodes,
        diarization=bool(regions),
        speaker_model="speechbrain/spkrec-ecapa-voxceleb" if regions else None,
        speaker_names=names,
        speaker_regions=regions,
        segments=segments_from_tokens(tokens, regions),
        podcast=normalize_podcast_metadata(podcast),
    )


def segments_from_tokens(tokens: list[WordToken], regions: list[SpeakerRegion]) -> list[TranscriptSegment]:
    if not tokens:
        return []

    assigned: list[tuple[WordToken, str | None]] = []
    for token in tokens:
        speaker = nearest_speaker((token.start + token.end) / 2, regions) if regions else None
        assigned.append((token, speaker))

    groups: list[list[tuple[WordToken, str | None]]] = []
    current: list[tuple[WordToken, str | None]] = []
    for index, item in enumerate(assigned):
        token, speaker = item
        if current:
            previous = current[-1][0]
            duration = token.end - current[0][0].start
            speaker_changed = speaker != current[-1][1]
            long_pause = token.start - previous.end > 0.9
            natural_end = previous.text.rstrip().endswith((".", "!", "?")) and duration >= 3.0
            punctuation_only = not any(character.isalnum() for character in token.text)
            if not punctuation_only and (speaker_changed or long_pause or duration > 14.0 or natural_end):
                groups.append(current)
                current = []
        current.append(item)
        if index == len(assigned) - 1 and current:
            groups.append(current)

    result = []
    for group in groups:
        text = tokens_to_text([item[0] for item in group])
        if text:
            final_token = group[-1][0]
            content_tokens = [item[0] for item in group if any(character.isalnum() for character in item[0].text)]
            end = content_tokens[-1].end if content_tokens else final_token.end
            if not any(character.isalnum() for character in final_token.text):
                end = max(end, final_token.start)
            result.append(
                TranscriptSegment(
                    id=str(uuid.uuid4()),
                    start=group[0][0].start,
                    end=end,
                    speaker=group[0][1],
                    text=text,
                )
            )
    return result


def nearest_speaker(moment: float, regions: list[SpeakerRegion]) -> str | None:
    if not regions:
        return None
    best = regions[0]
    best_distance = float("inf")
    for region in regions:
        if region.start <= moment <= region.end:
            return region.speaker
        distance = region.start - moment if moment < region.start else moment - region.end
        if distance < best_distance:
            best = region
            best_distance = distance
    return best.speaker


def tokens_to_text(tokens: list[WordToken]) -> str:
    raw = "".join(token.text for token in tokens).strip()
    if " " not in raw and len(tokens) > 1:
        raw = " ".join(token.text.strip() for token in tokens)
    return re.sub(r"\s+", " ", raw).strip()


def write_document(document: TranscriptDocument, directory: Path) -> Path:
    directory.mkdir(parents=True, exist_ok=True)
    path = directory / f"{document.id}.transcript.json"
    atomic_write(path, json.dumps(document.to_dict(), ensure_ascii=False, indent=2) + "\n")
    return path


def read_document(path: Path) -> TranscriptDocument:
    return TranscriptDocument.from_dict(json.loads(path.read_text(encoding="utf-8")))


def render_outputs(document: TranscriptDocument, output_dir: Path, formats: set[str], overwrite: bool = False) -> dict[str, str]:
    unknown = formats - FORMATS.keys()
    if unknown:
        raise ValueError(f"Unbekannte Ausgabeformate: {', '.join(sorted(unknown))}")
    output_dir.mkdir(parents=True, exist_ok=True)
    base = safe_output_base(output_dir, Path(document.source_file).stem, formats) if not overwrite else Path(document.source_file).stem
    renderers = {
        "markdown": render_markdown,
        "vtt": render_vtt,
        "srt": render_srt,
        "txt": render_txt,
        "csv": render_csv,
    }
    paths: dict[str, str] = {}
    for output_format in sorted(formats):
        path = output_dir / f"{base}{FORMATS[output_format]}"
        atomic_write(path, renderers[output_format](document))
        paths[output_format] = str(path)
    document.outputs = paths
    return paths


def render_existing_outputs(document: TranscriptDocument, formats: set[str] | None = None) -> dict[str, str]:
    selected = formats or set(document.outputs)
    renderers = {
        "markdown": render_markdown,
        "vtt": render_vtt,
        "srt": render_srt,
        "txt": render_txt,
        "csv": render_csv,
    }
    paths: dict[str, str] = {}
    for output_format in sorted(selected):
        existing = document.outputs.get(output_format)
        if not existing:
            continue
        path = Path(existing)
        atomic_write(path, renderers[output_format](document))
        paths[output_format] = str(path)
    return paths


def safe_output_base(directory: Path, preferred: str, formats: set[str]) -> str:
    candidate = preferred
    counter = 2
    while any((directory / f"{candidate}{FORMATS[item]}").exists() for item in formats):
        candidate = f"{preferred}_{counter}"
        counter += 1
    return candidate


def atomic_write(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, temporary = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent)
    try:
        with os.fdopen(fd, "w", encoding="utf-8", newline="") as handle:
            handle.write(content)
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temporary, path)
    except Exception:
        Path(temporary).unlink(missing_ok=True)
        raise


def display_speaker(document: TranscriptDocument, label: str | None) -> str:
    if not label:
        return ""
    return document.speaker_names.get(label, label)


def render_markdown(document: TranscriptDocument) -> str:
    speakers = [display_speaker(document, key) for key in sorted(document.speaker_names)]
    frontmatter = [
        "---",
        f"created: {yaml_quote(document.created)}",
        f"model: {yaml_quote(document.model)}",
        f"device: {yaml_quote(document.device)}",
        f"source_file: {yaml_quote(document.source_file)}",
        f"fps_timecode: {document.fps_timecode}",
        f"timecodes: {str(document.timecodes).lower()}",
        f"language: {yaml_quote(document.language)}",
        f"engine: {yaml_quote(document.engine)}",
        f"diarization: {str(document.diarization).lower()}",
        f"speaker_count: {len(document.speaker_names)}",
        f"speaker_model: {yaml_quote(document.speaker_model or '')}",
        f"speakers: {json.dumps(speakers, ensure_ascii=False)}",
    ]
    if document.podcast:
        frontmatter.append('source_type: "podcast"')
        frontmatter.append("podcast:")
        for key in PODCAST_YAML_FIELDS:
            value = document.podcast.get(key)
            if value is None or value == "" or value == []:
                continue
            frontmatter.append(f"  {key}: {yaml_value(value)}")
    frontmatter.extend(["---", ""])
    if document.podcast:
        frontmatter.extend(podcast_markdown_block(document))
    frontmatter.extend([f"# Transkript: {Path(document.source_file).stem}", "", "---", ""])
    blocks = []
    for segment in document.segments:
        lines = []
        if document.timecodes:
            lines.append(f"{fps_timecode(segment.start, document.fps_timecode)} - {fps_timecode(segment.end, document.fps_timecode)}")
        speaker = display_speaker(document, segment.speaker)
        lines.append(f"**{speaker}:** {segment.text}" if speaker else segment.text)
        blocks.append("\n".join(lines))
    return "\n".join(frontmatter) + "\n\n".join(blocks).strip() + "\n"


def render_vtt(document: TranscriptDocument) -> str:
    lines = ["WEBVTT", ""]
    for index, segment in enumerate(document.segments, start=1):
        speaker = display_speaker(document, segment.speaker)
        text = f"{speaker}: {segment.text}" if speaker else segment.text
        lines.extend([str(index), f"{vtt_time(segment.start)} --> {vtt_time(segment.end)}", text, ""])
    return "\n".join(lines)


def render_srt(document: TranscriptDocument) -> str:
    lines: list[str] = []
    for index, segment in enumerate(document.segments, start=1):
        speaker = display_speaker(document, segment.speaker)
        text = f"{speaker}: {segment.text}" if speaker else segment.text
        lines.extend([str(index), f"{srt_time(segment.start)} --> {srt_time(segment.end)}", text, ""])
    return "\n".join(lines)


def render_txt(document: TranscriptDocument) -> str:
    return "\n\n".join(
        f"{display_speaker(document, segment.speaker)}: {segment.text}" if segment.speaker else segment.text
        for segment in document.segments
    ).strip() + "\n"


def render_csv(document: TranscriptDocument) -> str:
    output = StringIO()
    writer = csv.writer(output, lineterminator="\n")
    writer.writerow(["start", "end", "speaker", "text"])
    for segment in document.segments:
        writer.writerow([csv_time(segment.start), csv_time(segment.end), display_speaker(document, segment.speaker), segment.text])
    return output.getvalue()


def yaml_quote(value: str) -> str:
    return json.dumps(value, ensure_ascii=False)


PODCAST_YAML_FIELDS = [
    "feed_url",
    "podcast_index_feed_id",
    "show_title",
    "episode_title",
    "author",
    "publisher",
    "language",
    "published_at",
    "downloaded_at",
    "episode_number",
    "season_number",
    "episode_type",
    "guid",
    "episode_url",
    "audio_url",
    "duration_seconds",
    "explicit",
    "image_url",
    "categories",
    "show_description",
    "episode_description",
]


def yaml_value(value: object) -> str:
    if isinstance(value, bool):
        return str(value).lower()
    if isinstance(value, (int, float)):
        return str(value)
    return json.dumps(value, ensure_ascii=False)


def normalize_podcast_metadata(podcast: dict | None) -> dict | None:
    if not isinstance(podcast, dict):
        return None
    result = {key: value for key, value in podcast.items() if key in PODCAST_YAML_FIELDS}
    for key in ("show_description", "episode_description"):
        value = result.get(key)
        if isinstance(value, str):
            result[key] = plain_text(value)
    return result or None


def plain_text(value: str) -> str:
    without_tags = re.sub(r"<[^>]+>", " ", value)
    return re.sub(r"\s+", " ", html.unescape(without_tags)).strip()


def podcast_markdown_block(document: TranscriptDocument) -> list[str]:
    podcast = document.podcast or {}
    rows = [
        ("Audiodatei", document.source_file),
        ("Show", podcast.get("show_title")),
        ("Episode", podcast.get("episode_title")),
        ("Autor:in", podcast.get("author")),
        ("Publisher", podcast.get("publisher")),
        ("Veröffentlicht", podcast.get("published_at")),
        ("Sprache", podcast.get("language")),
        ("RSS-Feed", podcast.get("feed_url")),
        ("Episodenlink", podcast.get("episode_url")),
    ]
    lines = ["## Podcast", ""]
    lines.extend(f"- **{label}:** {value}" for label, value in rows if value is not None and value != "")
    lines.extend(["", "---", ""])
    return lines


def fps_timecode(seconds: float, fps: int = 25) -> str:
    total = int(round(max(0.0, seconds) * fps))
    hours, total = divmod(total, 3600 * fps)
    minutes, total = divmod(total, 60 * fps)
    secs, frames = divmod(total, fps)
    return f"{hours:02d}:{minutes:02d}:{secs:02d}:{frames:02d}"


def vtt_time(seconds: float) -> str:
    millis = int(round(max(0.0, seconds) * 1000))
    hours, millis = divmod(millis, 3_600_000)
    minutes, millis = divmod(millis, 60_000)
    secs, millis = divmod(millis, 1000)
    return f"{hours:02d}:{minutes:02d}:{secs:02d}.{millis:03d}"


def srt_time(seconds: float) -> str:
    return vtt_time(seconds).replace(".", ",")


def csv_time(seconds: float) -> str:
    millis = int(round(max(0.0, seconds) * 1000))
    hours, millis = divmod(millis, 3_600_000)
    minutes, millis = divmod(millis, 60_000)
    secs, millis = divmod(millis, 1000)
    return f"{hours:02d}:{minutes:02d}:{secs:02d}.{millis:03d}"


def apply_replacements(document: TranscriptDocument, replacements: dict[str, str]) -> None:
    for source, replacement in replacements.items():
        source = source.strip()
        if not source or source == replacement:
            continue
        pattern = re.compile(rf"(?<!\w){re.escape(source)}(?!\w)", re.IGNORECASE)
        for segment in document.segments:
            segment.text = pattern.sub(replacement.strip(), segment.text)
