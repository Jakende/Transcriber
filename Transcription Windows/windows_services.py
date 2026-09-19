from __future__ import annotations

import hashlib
import html
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import threading
import urllib.parse
import urllib.request
import uuid
import xml.etree.ElementTree as ET
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Callable, Iterable


APP_TITLE = "Transcription Windows"
SUPPORTED_EXTENSIONS = {
    ".wav", ".mp3", ".m4a", ".flac", ".aac", ".ogg", ".wma",
    ".mp4", ".mov", ".m4v", ".avi", ".mkv", ".webm", ".mpeg",
    ".mpg", ".mts", ".m2ts", ".ts", ".vob", ".flv", ".3gp", ".wmv",
}


def _shared_backend_root() -> Path:
    return (
        Path(__file__).resolve().parents[1]
        / "Transcription macOS App"
        / "Sources"
        / "TranscriptionMacOSApp"
        / "Resources"
    )


if not getattr(sys, "frozen", False):
    backend_root = _shared_backend_root()
    if str(backend_root) not in sys.path:
        sys.path.insert(0, str(backend_root))

from transcription_backend.document import (  # noqa: E402
    apply_replacements,
    build_document,
    read_document,
    render_existing_outputs,
    render_outputs,
    write_document,
)
from transcription_backend.glossary import extract_candidates  # noqa: E402
from transcription_backend.media import normalize_audio  # noqa: E402
from transcription_backend.models import TranscriptDocument, TranscriptSegment, WordToken  # noqa: E402


def bundled_resource_path(name: str) -> Path:
    if getattr(sys, "frozen", False) and hasattr(sys, "_MEIPASS"):
        return Path(sys._MEIPASS) / name
    return Path(__file__).resolve().parent / name


def app_data_dir() -> Path:
    base = os.environ.get("LOCALAPPDATA") or os.environ.get("APPDATA") or str(Path.home())
    path = Path(base) / APP_TITLE
    path.mkdir(parents=True, exist_ok=True)
    return path


def jobs_dir() -> Path:
    path = app_data_dir() / "Jobs"
    path.mkdir(parents=True, exist_ok=True)
    return path


def backups_dir() -> Path:
    path = app_data_dir() / "Backups"
    path.mkdir(parents=True, exist_ok=True)
    return path


def models_dir() -> Path:
    path = app_data_dir() / "models"
    path.mkdir(parents=True, exist_ok=True)
    return path


def speaker_models_dir() -> Path:
    path = models_dir() / "speaker"
    path.mkdir(parents=True, exist_ok=True)
    return path


def configure_bundled_tools() -> dict[str, Path | None]:
    tools: dict[str, Path | None] = {}
    for name in ("ffmpeg", "ffprobe", "ffplay", "deno"):
        filename = f"{name}.exe"
        bundled = bundled_resource_path(filename)
        vendor = Path(__file__).resolve().parent / "vendor" / ("deno" if name == "deno" else "ffmpeg") / filename
        found = bundled if bundled.exists() else vendor if vendor.exists() else (Path(value) if (value := shutil.which(name)) else None)
        tools[name] = found
    directories = {str(path.parent) for path in tools.values() if path is not None}
    if directories:
        os.environ["PATH"] = os.pathsep.join(sorted(directories)) + os.pathsep + os.environ.get("PATH", "")
    return tools


TOOLS = configure_bundled_tools()


def atomic_json_write(path: Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.{uuid.uuid4().hex}.tmp")
    temporary.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    os.replace(temporary, path)


def load_json(path: Path, default: Any) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return default


def is_supported_media(path: Path) -> bool:
    return path.is_file() and path.suffix.casefold() in SUPPORTED_EXTENSIONS


@dataclass
class QueueItem:
    path: str
    podcast: dict[str, Any] | None = None

    @property
    def source(self) -> Path:
        return Path(self.path)


@dataclass
class TranscriptionOptions:
    language: str = "de"
    model: str = "turbo"
    include_timecodes: bool = True
    diarize: bool = True
    speaker_range: str = "auto"
    cluster_threshold: float = 0.60
    formats: set[str] = field(default_factory=lambda: {"markdown"})
    output_dir: Path | None = None


class WindowsStore:
    def __init__(self, root: Path | None = None) -> None:
        self.root = root or app_data_dir()
        self.root.mkdir(parents=True, exist_ok=True)
        self.queue_path = self.root / "PendingQueue.json"
        self.preferences_path = self.root / "Preferences.json"
        self.library_path = self.root / "Library.json"
        self.glossary_path = self.root / "Glossary.json"
        self.podcast_registry_path = self.root / "PodcastDownloads.json"

    def load_queue(self) -> list[QueueItem]:
        items = []
        for raw in load_json(self.queue_path, []):
            try:
                item = QueueItem(path=str(raw["path"]), podcast=raw.get("podcast"))
            except (KeyError, TypeError):
                continue
            if is_supported_media(item.source):
                items.append(item)
        return items

    def save_queue(self, items: Iterable[QueueItem]) -> None:
        atomic_json_write(self.queue_path, [item.__dict__ for item in items])

    def load_preferences(self) -> dict[str, Any]:
        return load_json(self.preferences_path, {})

    def save_preferences(self, values: dict[str, Any]) -> None:
        atomic_json_write(self.preferences_path, values)

    def archived_ids(self) -> set[str]:
        raw = load_json(self.library_path, {})
        return set(raw.get("archived", [])) if isinstance(raw, dict) else set()

    def save_archived_ids(self, values: set[str]) -> None:
        atomic_json_write(self.library_path, {"archived": sorted(values)})

    def glossary_terms(self) -> list[str]:
        raw = load_json(self.glossary_path, [])
        return sorted({str(item).strip() for item in raw if str(item).strip()}, key=str.casefold)

    def save_glossary_terms(self, values: Iterable[str]) -> None:
        terms = sorted({value.strip() for value in values if value.strip()}, key=str.casefold)
        atomic_json_write(self.glossary_path, terms)

    def glossary_prompt(self, max_characters: int = 1000) -> str | None:
        prefix = "Wichtige Namen und Fachbegriffe: "
        result = prefix
        for term in self.glossary_terms():
            addition = ("" if result == prefix else ", ") + term
            if len(result) + len(addition) > max_characters:
                break
            result += addition
        return result if result != prefix else None

    def podcast_records(self) -> dict[str, dict[str, Any]]:
        raw = load_json(self.podcast_registry_path, {})
        return raw if isinstance(raw, dict) else {}

    def podcast_metadata(self, path: Path) -> dict[str, Any] | None:
        return self.podcast_records().get(str(path.resolve()))

    def register_podcast(self, path: Path, metadata: dict[str, Any]) -> None:
        records = self.podcast_records()
        records[str(path.resolve())] = metadata
        atomic_json_write(self.podcast_registry_path, records)


@dataclass
class ResultRecord:
    document_path: Path
    source_path: Path
    outputs: dict[str, str]
    speaker_count: int
    segment_count: int
    document_id: str


def result_from_document(path: Path) -> ResultRecord:
    document = read_document(path)
    return ResultRecord(
        document_path=path,
        source_path=Path(document.source_path),
        outputs=document.outputs,
        speaker_count=len(set(document.speaker_names) | {item.speaker for item in document.speaker_regions}),
        segment_count=len(document.segments),
        document_id=document.id,
    )


def load_results(directory: Path | None = None) -> list[ResultRecord]:
    root = directory or jobs_dir()
    results = []
    for path in root.glob("*.transcript.json"):
        try:
            results.append(result_from_document(path))
        except (OSError, ValueError, TypeError, json.JSONDecodeError):
            continue
    return sorted(results, key=lambda item: item.document_path.stat().st_mtime, reverse=True)


def create_document_backup(document_path: Path, minimum_interval: float = 30.0, keep: int = 20) -> None:
    if not document_path.exists():
        return
    directory = backups_dir() / document_path.name
    directory.mkdir(parents=True, exist_ok=True)
    existing = sorted(directory.glob("*.json"), reverse=True)
    if existing:
        age = datetime.now().timestamp() - existing[0].stat().st_mtime
        if age < minimum_interval:
            return
    stamp = datetime.now().strftime("%Y%m%d-%H%M%S-%f")
    shutil.copy2(document_path, directory / f"{stamp}.json")
    for stale in sorted(directory.glob("*.json"), reverse=True)[keep:]:
        stale.unlink(missing_ok=True)


def save_document(document: TranscriptDocument, document_path: Path, backup: bool = True) -> None:
    if backup:
        create_document_backup(document_path)
    atomic_json_write(document_path, document.to_dict())


def delete_internal_record(path: Path) -> None:
    try:
        from send2trash import send2trash

        send2trash(str(path))
    except ImportError:
        trash = app_data_dir() / "Trash"
        trash.mkdir(parents=True, exist_ok=True)
        destination = trash / path.name
        counter = 2
        while destination.exists():
            destination = trash / f"{path.stem}_{counter}{path.suffix}"
            counter += 1
        shutil.move(str(path), destination)


def _word_tokens(result: dict[str, Any]) -> list[WordToken]:
    tokens: list[WordToken] = []
    for segment in result.get("segments", []):
        words = segment.get("words") or []
        if words:
            for word in words:
                text = str(word.get("word") or "")
                if text.strip():
                    tokens.append(WordToken(float(word.get("start", 0)), float(word.get("end", 0)), text))
        else:
            text = str(segment.get("text") or "")
            if text.strip():
                tokens.append(WordToken(float(segment.get("start", 0)), float(segment.get("end", 0)), text))
    return tokens


class Transcriber:
    def __init__(self, log: Callable[[str, str], None], progress: Callable[[int, str], None]) -> None:
        self.log = log
        self.progress = progress
        self._model: Any = None
        self._model_name: str | None = None
        self._device = "cpu"

    @property
    def device(self) -> str:
        return self._device

    def load_model(self, model_name: str) -> None:
        import torch
        import whisper

        if self._model is not None and self._model_name == model_name:
            return
        preferred = "cuda" if torch.cuda.is_available() else "cpu"
        self.log(f"Lade Whisper-Modell „{model_name}“ für {preferred} …", "info")
        model = whisper.load_model(model_name, device="cpu", download_root=str(models_dir()))
        if preferred == "cuda":
            try:
                model = model.to("cuda")
                self._device = "cuda"
            except Exception as error:
                self.log(f"CUDA konnte nicht verwendet werden; CPU wird genutzt: {error}", "error")
                self._device = "cpu"
        else:
            self._device = "cpu"
            try:
                torch.set_num_threads(max(1, os.cpu_count() or 1))
            except RuntimeError:
                pass
        self._model = model
        self._model_name = model_name

    def transcribe_one(
        self,
        item: QueueItem,
        options: TranscriptionOptions,
        store: WindowsStore,
        cancel: threading.Event,
    ) -> ResultRecord:
        if cancel.is_set():
            raise InterruptedError("Abgebrochen")
        source = item.source.resolve()
        if not is_supported_media(source):
            raise ValueError(f"Nicht unterstützte oder fehlende Datei: {source}")
        ffmpeg = TOOLS.get("ffmpeg")
        if ffmpeg is None:
            raise FileNotFoundError("ffmpeg wurde nicht gefunden.")

        self.progress(3, "Bereite Audio vor …")
        with tempfile.TemporaryDirectory(prefix="transcription-windows-") as temporary:
            wav_path = Path(temporary) / "audio.wav"
            normalize_audio(source, wav_path, ffmpeg)
            regions = []
            if options.diarize:
                from transcription_backend.diarization import diarize_audio, parse_speaker_range

                self.progress(10, "Erkenne Sprecher:innen …")
                minimum, maximum = parse_speaker_range(options.speaker_range)
                regions = diarize_audio(
                    wav_path,
                    speaker_models_dir(),
                    min_speakers=minimum,
                    max_speakers=maximum,
                    threshold=options.cluster_threshold,
                )
                self.progress(35, f"{len({region.speaker for region in regions})} Sprecher:innen erkannt")
            if cancel.is_set():
                raise InterruptedError("Abgebrochen")

            self.progress(40 if options.diarize else 12, "Transkribiere …")
            language = None if options.language == "auto" else options.language
            result = self._model.transcribe(
                str(wav_path),
                language=language,
                verbose=False,
                fp16=self._device == "cuda",
                beam_size=1,
                best_of=1,
                temperature=0,
                condition_on_previous_text=False,
                word_timestamps=True,
                initial_prompt=store.glossary_prompt(),
            )
            if cancel.is_set():
                raise InterruptedError("Abgebrochen")
            self.progress(88, "Führe Text und Sprecher:innen zusammen …")
            document = build_document(
                source,
                options.language,
                options.model,
                _word_tokens(result),
                regions,
                options.include_timecodes,
                podcast=item.podcast,
            )
            document.device = self._device
            document.engine = "openai-whisper"
            annotate_segment_languages(document)
            destination = options.output_dir or source.parent
            outputs = render_outputs(document, destination, options.formats)
            document_path = write_document(document, jobs_dir())
            self.progress(100, "Gespeichert")
            return ResultRecord(
                document_path=document_path,
                source_path=source,
                outputs=outputs,
                speaker_count=len(document.speaker_names),
                segment_count=len(document.segments),
                document_id=document.id,
            )


GERMAN_HINTS = {"der", "die", "das", "und", "ist", "nicht", "mit", "für", "auf", "ein", "eine", "wir", "sie"}
ENGLISH_HINTS = {"the", "and", "is", "not", "with", "for", "on", "a", "an", "we", "you", "they"}


def detect_segment_language(text: str) -> str | None:
    words = re.findall(r"[A-Za-zÄÖÜäöüß']+", text.casefold())
    if len("".join(words)) < 12:
        return None
    german = sum(word in GERMAN_HINTS for word in words) + sum(any(char in word for char in "äöüß") for word in words)
    english = sum(word in ENGLISH_HINTS for word in words)
    if german == english == 0:
        return None
    return "de" if german >= english else "en"


def annotate_segment_languages(document: TranscriptDocument) -> None:
    for segment in document.segments:
        if segment.language is None:
            segment.language = detect_segment_language(segment.text)


def split_segment(document: TranscriptDocument, segment_id: str) -> None:
    index = next((index for index, item in enumerate(document.segments) if item.id == segment_id), None)
    if index is None:
        return
    segment = document.segments[index]
    words = segment.text.split()
    if len(words) < 2 or segment.end <= segment.start:
        return
    split_at = max(1, len(words) // 2)
    midpoint = segment.start + (segment.end - segment.start) * split_at / len(words)
    first = TranscriptSegment(segment.id, segment.start, midpoint, " ".join(words[:split_at]), segment.speaker, segment.language)
    second = TranscriptSegment(str(uuid.uuid4()), midpoint, segment.end, " ".join(words[split_at:]), segment.speaker, segment.language)
    document.segments[index:index + 1] = [first, second]


def merge_segment_with_next(document: TranscriptDocument, segment_id: str) -> None:
    index = next((index for index, item in enumerate(document.segments) if item.id == segment_id), None)
    if index is None or index + 1 >= len(document.segments):
        return
    current = document.segments[index]
    following = document.segments[index + 1]
    current.end = max(current.end, following.end)
    current.text = " ".join(value for value in (current.text.strip(), following.text.strip()) if value)
    current.speaker = current.speaker or following.speaker
    current.language = current.language or following.language
    document.segments.pop(index + 1)


def speaker_labels(document: TranscriptDocument) -> list[str]:
    labels = set(document.speaker_names)
    labels.update(region.speaker for region in document.speaker_regions)
    labels.update(segment.speaker for segment in document.segments if segment.speaker)
    return sorted(labels)


def merge_speakers(document: TranscriptDocument, source: str, target: str) -> None:
    if not source or not target or source == target:
        return
    for segment in document.segments:
        if segment.speaker == source:
            segment.speaker = target
    for region in document.speaker_regions:
        if region.speaker == source:
            region.speaker = target
    document.speaker_names.setdefault(target, document.speaker_names.get(source, target))
    document.speaker_names.pop(source, None)
    merged = []
    for region in sorted(document.speaker_regions, key=lambda item: (item.start, item.end)):
        if merged and merged[-1].speaker == region.speaker and region.start <= merged[-1].end + 0.5:
            merged[-1].end = max(merged[-1].end, region.end)
        else:
            merged.append(region)
    document.speaker_regions = merged
    document.diarization = bool(speaker_labels(document))


def parse_vtt(vtt_path: Path, audio_path: Path | None = None) -> TranscriptDocument:
    content = vtt_path.read_text(encoding="utf-8-sig").replace("\r\n", "\n")
    segments: list[TranscriptSegment] = []
    speaker_names: dict[str, str] = {}
    labels: dict[str, str] = {}
    timing_pattern = re.compile(r"(?P<start>\d{1,2}:\d{2}(?::\d{2})?[.,]\d{3})\s+-->\s+(?P<end>\d{1,2}:\d{2}(?::\d{2})?[.,]\d{3})")
    for block in re.split(r"\n\s*\n", content):
        lines = [line.strip() for line in block.splitlines() if line.strip()]
        timing_index = next((index for index, line in enumerate(lines) if "-->" in line), None)
        if timing_index is None:
            continue
        match = timing_pattern.search(lines[timing_index])
        if not match:
            continue
        text = html.unescape(re.sub(r"<[^>]+>", "", " ".join(lines[timing_index + 1:]))).strip()
        if not text:
            continue
        speaker = None
        if ":" in text:
            candidate, remainder = text.split(":", 1)
            if 0 < len(candidate.strip()) <= 60 and "." not in candidate:
                name = candidate.strip()
                speaker = labels.setdefault(name, f"SPEAKER_{len(labels):02d}")
                speaker_names[speaker] = name
                text = remainder.strip()
        segments.append(TranscriptSegment(str(uuid.uuid4()), _vtt_seconds(match["start"]), _vtt_seconds(match["end"]), text, speaker))
    if not segments:
        raise ValueError("Die VTT-Datei enthält keine erkennbaren Untertitel.")
    source = audio_path or vtt_path
    document = TranscriptDocument(
        id=str(uuid.uuid4()),
        source_path=str(source.resolve()),
        source_file=f"{vtt_path.stem}_bearbeitet.vtt",
        language="de",
        model="importiert",
        device="nicht zutreffend",
        engine="VTT-Import",
        created=datetime.now().strftime("%Y-%m-%d %H:%M"),
        fps_timecode=25,
        timecodes=True,
        diarization=bool(speaker_names),
        speaker_names=speaker_names,
        segments=segments,
    )
    annotate_segment_languages(document)
    return document


def _vtt_seconds(value: str) -> float:
    parts = value.replace(",", ".").split(":")
    if len(parts) == 3:
        return int(parts[0]) * 3600 + int(parts[1]) * 60 + float(parts[2])
    return int(parts[0]) * 60 + float(parts[1])


@dataclass
class PodcastEpisode:
    id: str
    title: str
    audio_url: str
    published_at: str | None = None
    author: str | None = None
    episode_number: int | None = None
    season_number: int | None = None
    duration_seconds: int | None = None
    description: str | None = None
    episode_url: str | None = None
    image_url: str | None = None
    explicit: bool | None = None


@dataclass
class PodcastFeed:
    feed_url: str
    title: str
    episodes: list[PodcastEpisode]
    author: str | None = None
    publisher: str | None = None
    language: str | None = None
    description: str | None = None
    image_url: str | None = None
    categories: list[str] = field(default_factory=list)
    podcast_index_feed_id: int | None = None


def _download_bounded(url: str, limit: int, headers: dict[str, str] | None = None) -> tuple[bytes, str]:
    parsed = urllib.parse.urlparse(url)
    if parsed.scheme not in {"http", "https"}:
        raise ValueError("Zulässig sind nur HTTP- oder HTTPS-Adressen.")
    request = urllib.request.Request(url, headers={"User-Agent": "Transcription-Windows/2.1", **(headers or {})})
    with urllib.request.urlopen(request, timeout=60) as response:
        final_url = response.geturl()
        if urllib.parse.urlparse(final_url).scheme != "https":
            raise ValueError("Die Ressource muss HTTPS verwenden oder dorthin weiterleiten.")
        advertised = response.headers.get("Content-Length")
        if advertised and int(advertised) > limit:
            raise ValueError("Die Serverantwort ist zu groß.")
        data = response.read(limit + 1)
        if len(data) > limit:
            raise ValueError("Die Serverantwort ist zu groß.")
        return data, final_url


def _texts(element: ET.Element, local_name: str) -> list[str]:
    values = []
    for child in element.iter():
        if child.tag.rsplit("}", 1)[-1].casefold() == local_name.casefold() and child.text:
            value = child.text.strip()
            if value:
                values.append(value)
    return values


def _first_text(element: ET.Element, *names: str) -> str | None:
    for name in names:
        values = _texts(element, name)
        if values:
            return values[0]
    return None


def _integer(value: str | None) -> int | None:
    if not value:
        return None
    try:
        return int(value)
    except ValueError:
        if ":" in value:
            try:
                parts = [int(item) for item in value.split(":")]
                return sum(item * (60 ** index) for index, item in enumerate(reversed(parts)))
            except ValueError:
                return None
        return None


def _clean_html(value: str | None) -> str | None:
    if not value:
        return None
    return re.sub(r"\s+", " ", html.unescape(re.sub(r"<[^>]+>", " ", value))).strip() or None


def load_rss_feed(url: str, podcast_index_feed_id: int | None = None) -> PodcastFeed:
    data, final_url = _download_bounded(url, 12_000_000)
    root = ET.fromstring(data)
    channel = next((node for node in root.iter() if node.tag.rsplit("}", 1)[-1].casefold() in {"channel", "feed"}), root)
    title = _first_text(channel, "title") or "Podcast"
    episodes: list[PodcastEpisode] = []
    entries = [node for node in channel if node.tag.rsplit("}", 1)[-1].casefold() in {"item", "entry"}]
    for index, item in enumerate(entries):
        enclosure = next((node for node in item.iter() if node.tag.rsplit("}", 1)[-1].casefold() == "enclosure" and node.attrib.get("url")), None)
        audio_url = enclosure.attrib.get("url", "").strip() if enclosure is not None else ""
        if not audio_url:
            continue
        episode_title = _first_text(item, "title") or f"Episode {index + 1}"
        guid = _first_text(item, "guid", "id") or audio_url
        episodes.append(PodcastEpisode(
            id=guid,
            title=episode_title,
            audio_url=audio_url,
            published_at=_first_text(item, "pubDate", "published", "updated"),
            author=_first_text(item, "author", "creator"),
            episode_number=_integer(_first_text(item, "episode")),
            season_number=_integer(_first_text(item, "season")),
            duration_seconds=_integer(_first_text(item, "duration")),
            description=_clean_html(_first_text(item, "description", "summary", "content")),
            episode_url=_first_text(item, "link"),
            image_url=next((node.attrib.get("href") for node in item.iter() if node.tag.rsplit("}", 1)[-1].casefold() == "image" and node.attrib.get("href")), None),
            explicit=(_first_text(item, "explicit") or "").casefold() in {"yes", "true", "explicit"},
        ))
    if not episodes:
        raise ValueError("Der RSS-Feed enthält keine Folgen mit Audio-Enclosure.")
    categories = sorted(set(_texts(channel, "category")), key=str.casefold)
    return PodcastFeed(
        feed_url=final_url,
        title=title,
        episodes=episodes,
        author=_first_text(channel, "author", "managingEditor", "creator"),
        publisher=_first_text(channel, "owner"),
        language=_first_text(channel, "language"),
        description=_clean_html(_first_text(channel, "description", "summary", "subtitle")),
        image_url=_first_text(channel, "image"),
        categories=categories,
        podcast_index_feed_id=podcast_index_feed_id,
    )


def podcast_index_search(term: str, key: str, secret: str) -> list[dict[str, Any]]:
    if not key.strip() or not secret.strip():
        raise ValueError("PodcastIndex-Zugangsdaten fehlen.")
    timestamp = str(int(datetime.now(tz=timezone.utc).timestamp()))
    signature = hashlib.sha1(f"{key}{secret}{timestamp}".encode()).hexdigest()
    query = urllib.parse.urlencode({"q": term.strip(), "max": 40})
    headers = {"X-Auth-Key": key.strip(), "X-Auth-Date": timestamp, "Authorization": signature}
    data, _ = _download_bounded(f"https://api.podcastindex.org/api/1.0/search/byterm?{query}", 5_000_000, headers)
    payload = json.loads(data)
    return list(payload.get("feeds") or [])


def save_podcast_credentials(key: str, secret: str) -> None:
    try:
        import keyring

        keyring.set_password(APP_TITLE, "PODCAST_INDEX_KEY", key.strip())
        keyring.set_password(APP_TITLE, "PODCAST_INDEX_SECRET", secret.strip())
    except ImportError as error:
        raise RuntimeError("Das Paket keyring fehlt; Zugangsdaten konnten nicht sicher gespeichert werden.") from error


def load_podcast_credentials() -> tuple[str, str]:
    env_key = os.environ.get("PODCAST_INDEX_KEY", "")
    env_secret = os.environ.get("PODCAST_INDEX_SECRET", "")
    try:
        import keyring

        return keyring.get_password(APP_TITLE, "PODCAST_INDEX_KEY") or env_key, keyring.get_password(APP_TITLE, "PODCAST_INDEX_SECRET") or env_secret
    except ImportError:
        return env_key, env_secret


def safe_filename(value: str, maximum: int = 120) -> str:
    value = re.sub(r"[<>:\"/\\|?*\x00-\x1f]", " ", value)
    value = re.sub(r"\s+", " ", value).strip(" .")
    return (value or "Download")[:maximum].rstrip(" .")


def available_path(directory: Path, stem: str, suffix: str) -> Path:
    candidate = directory / f"{stem}{suffix}"
    counter = 2
    while candidate.exists():
        candidate = directory / f"{stem}_{counter}{suffix}"
        counter += 1
    return candidate


def download_podcast_episode(
    feed: PodcastFeed,
    episode: PodcastEpisode,
    directory: Path,
    progress: Callable[[float], None] | None = None,
) -> QueueItem:
    directory.mkdir(parents=True, exist_ok=True)
    parsed = urllib.parse.urlparse(episode.audio_url)
    suffix = Path(parsed.path).suffix.casefold()
    if suffix not in SUPPORTED_EXTENSIONS:
        suffix = ".mp3"
    date_prefix = ""
    if episode.published_at:
        match = re.search(r"\d{4}-\d{2}-\d{2}", episode.published_at)
        if match:
            date_prefix = match.group(0) + " – "
    destination = available_path(directory, safe_filename(f"{date_prefix}{feed.title} – {episode.title}"), suffix)
    request = urllib.request.Request(episode.audio_url, headers={"User-Agent": "Transcription-Windows/2.1"})
    with urllib.request.urlopen(request, timeout=120) as response, destination.open("wb") as handle:
        if urllib.parse.urlparse(response.geturl()).scheme != "https":
            raise ValueError("Die Audiodatei muss über HTTPS ausgeliefert werden.")
        total = int(response.headers.get("Content-Length") or 0)
        written = 0
        while True:
            chunk = response.read(1024 * 1024)
            if not chunk:
                break
            handle.write(chunk)
            written += len(chunk)
            if progress and total:
                progress(min(1.0, written / total))
    metadata = {
        "feed_url": feed.feed_url,
        "podcast_index_feed_id": feed.podcast_index_feed_id,
        "show_title": feed.title,
        "episode_title": episode.title,
        "author": episode.author or feed.author,
        "publisher": feed.publisher,
        "language": feed.language,
        "published_at": episode.published_at,
        "downloaded_at": datetime.now(tz=timezone.utc).isoformat(),
        "episode_number": episode.episode_number,
        "season_number": episode.season_number,
        "guid": episode.id,
        "episode_url": episode.episode_url,
        "audio_url": episode.audio_url,
        "duration_seconds": episode.duration_seconds,
        "explicit": episode.explicit,
        "image_url": episode.image_url or feed.image_url,
        "categories": feed.categories,
        "show_description": feed.description,
        "episode_description": episode.description,
    }
    return QueueItem(str(destination.resolve()), metadata)


YOUTUBE_HOSTS = {"youtube.com", "youtu.be", "youtube-nocookie.com"}


def validate_youtube_url(value: str) -> str:
    url = value.strip()
    parsed = urllib.parse.urlparse(url)
    host = (parsed.hostname or "").casefold()
    allowed = any(host == item or host.endswith(f".{item}") for item in YOUTUBE_HOSTS)
    query = urllib.parse.parse_qs(parsed.query)
    parts = [part for part in parsed.path.split("/") if part]
    single = (host == "youtu.be" and bool(parts)) or (parsed.path == "/watch" and bool(query.get("v"))) or (parts and parts[0] in {"shorts", "live", "embed"} and len(parts) >= 2)
    if parsed.scheme != "https" or not allowed or not single:
        raise ValueError("Bitte einen gültigen HTTPS-Link zu einem einzelnen YouTube-Video eingeben.")
    return url


def _youtube_options(progress: Callable[[float], None] | None = None) -> dict[str, Any]:
    options: dict[str, Any] = {"noplaylist": True, "quiet": True, "no_warnings": True}
    deno = TOOLS.get("deno")
    if deno:
        options["js_runtimes"] = {"deno": {"path": str(deno)}}
    if progress:
        def hook(event: dict[str, Any]) -> None:
            if event.get("status") == "downloading" and event.get("total_bytes"):
                progress(min(1.0, event.get("downloaded_bytes", 0) / event["total_bytes"]))
            elif event.get("status") == "finished":
                progress(1.0)
        options["progress_hooks"] = [hook]
    return options


def inspect_youtube(url: str) -> dict[str, Any]:
    from yt_dlp import YoutubeDL

    with YoutubeDL(_youtube_options()) as loader:
        info = loader.extract_info(validate_youtube_url(url), download=False)
    return {
        "id": str(info.get("id") or ""),
        "title": str(info.get("title") or ""),
        "channel": info.get("channel") or info.get("uploader"),
        "duration": info.get("duration"),
        "upload_date": info.get("upload_date"),
        "webpage_url": info.get("webpage_url") or url,
    }


def download_youtube(
    url: str,
    directory: Path,
    keep_video: bool,
    progress: Callable[[float], None] | None = None,
) -> tuple[QueueItem, Path | None]:
    from yt_dlp import YoutubeDL

    validated = validate_youtube_url(url)
    directory.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="youtube-download-") as temporary:
        work = Path(temporary)
        options = _youtube_options(progress)
        options.update({
            "outtmpl": str(work / "source.%(ext)s"),
            "format": "bestvideo*+bestaudio/best" if keep_video else "bestaudio/best",
        })
        if keep_video:
            options["merge_output_format"] = "mp4"
        with YoutubeDL(options) as loader:
            info = loader.extract_info(validated, download=True)
        media_files = [path for path in work.iterdir() if path.is_file() and path.suffix not in {".part", ".ytdl"}]
        if not media_files:
            raise RuntimeError("yt-dlp hat keine Mediendatei erzeugt.")
        downloaded = max(media_files, key=lambda path: path.stat().st_size)
        title = safe_filename(str(info.get("title") or "YouTube"))
        audio_destination = available_path(directory, title, ".mp3")
        ffmpeg = TOOLS.get("ffmpeg")
        if ffmpeg is None:
            raise FileNotFoundError("ffmpeg wurde nicht gefunden.")
        conversion = subprocess.run(
            [str(ffmpeg), "-nostdin", "-v", "error", "-y", "-i", str(downloaded), "-map", "0:a:0", "-vn", "-c:a", "libmp3lame", "-q:a", "2", str(audio_destination)],
            capture_output=True,
            text=True,
        )
        if conversion.returncode != 0:
            raise RuntimeError((conversion.stderr or "MP3-Konvertierung fehlgeschlagen")[-1200:])
        video_destination = None
        if keep_video:
            video_destination = available_path(directory, title, downloaded.suffix.casefold() or ".mp4")
            shutil.move(str(downloaded), video_destination)
    return QueueItem(str(audio_destination.resolve())), video_destination


def diagnostics_report() -> str:
    lines = [f"{APP_TITLE} – Systembericht", f"Python: {sys.version.split()[0]}", f"Programm: {sys.executable}"]
    for name, path in TOOLS.items():
        lines.append(f"{'OK' if path else 'FEHLT'}: {name} – {path or 'nicht gefunden'}")
    for module_name in ("torch", "whisper", "speechbrain", "silero_vad", "spacy", "yt_dlp", "keyring", "tkinterdnd2"):
        try:
            module = __import__(module_name)
            version = getattr(module, "__version__", "installiert")
            lines.append(f"OK: {module_name} – {version}")
        except Exception as error:
            lines.append(f"FEHLT: {module_name} – {error}")
    cached_models = sorted(path.name for path in models_dir().glob("*.pt"))
    speaker_model = speaker_models_dir() / "spkrec-ecapa-voxceleb" / "hyperparams.yaml"
    lines.extend([
        "",
        f"Modelle: {models_dir()}",
        f"Whisper-Cache: {', '.join(cached_models) if cached_models else 'noch leer'}",
        f"Sprechermodell: {'OK' if speaker_model.exists() else 'wird bei erster Nutzung geladen'}",
        f"Bearbeitungsstände: {len(list(jobs_dir().glob('*.transcript.json')))}",
        f"Speicherort: {app_data_dir()}",
    ])
    return "\n".join(lines)


def export_document(document: TranscriptDocument, directory: Path, formats: set[str]) -> dict[str, str]:
    outputs = render_outputs(document, directory, formats)
    return outputs


def refresh_existing_outputs(document: TranscriptDocument, formats: set[str] | None = None) -> dict[str, str]:
    return render_existing_outputs(document, formats)


def glossary_candidates(document: TranscriptDocument) -> list[dict[str, Any]]:
    return extract_candidates(" ".join(segment.text for segment in document.segments), document.language)


def replace_terms(document: TranscriptDocument, replacements: dict[str, str]) -> None:
    apply_replacements(document, replacements)
