#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
import shutil
import sys
import tempfile
from pathlib import Path

os.environ.setdefault("PYTORCH_ENABLE_MPS_FALLBACK", "1")
os.environ.setdefault("TOKENIZERS_PARALLELISM", "false")

from transcription_backend.diarization import diarize_audio, parse_speaker_range
from transcription_backend.document import (
    apply_replacements,
    build_document,
    read_document,
    render_existing_outputs,
    render_outputs,
    write_document,
)
from transcription_backend.events import emit
from transcription_backend.glossary import extract_candidates
from transcription_backend.media import normalize_audio, transcribe_words


MODEL_FILES = {
    "turbo": "ggml-large-v3-turbo.bin",
    "large-v3-turbo": "ggml-large-v3-turbo.bin",
    "medium": "ggml-medium.bin",
    "small": "ggml-small.bin",
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Offline-Transkription für die native macOS-App")
    parser.add_argument("--mode", choices=["transcribe", "render", "glossary"], default="transcribe")
    parser.add_argument("--file", action="append")
    parser.add_argument("--document")
    parser.add_argument("--language", choices=["auto", "de", "en"], default="de")
    parser.add_argument("--model", choices=sorted(MODEL_FILES), default="turbo")
    parser.add_argument("--diarize", action="store_true")
    parser.add_argument("--speaker-range", default="auto")
    parser.add_argument("--cluster-threshold", type=float, default=0.5)
    parser.add_argument("--timecodes", action="store_true")
    parser.add_argument("--format", action="append", dest="formats", choices=["markdown", "vtt", "srt", "txt", "csv"])
    parser.add_argument("--output-dir")
    parser.add_argument("--state-dir")
    parser.add_argument("--resource-root")
    parser.add_argument("--whisper-cli")
    parser.add_argument("--ffmpeg")
    parser.add_argument("--models-dir")
    parser.add_argument("--speaker-models-dir")
    parser.add_argument("--replace", action="append", default=[], help="Begriffsersetzung als Quelle=Ziel")
    parser.add_argument("--prompt", help="Kontext mit bestätigten Namen und Fachbegriffen")
    parser.add_argument("--source-metadata-file", help="Temporäre JSON-Datei mit Podcast-Metadaten nach Quellpfad")
    return parser.parse_args()


def resolve_executable(explicit: str | None, bundled: Path, name: str) -> Path:
    if explicit:
        candidate = Path(explicit)
    elif bundled.exists():
        candidate = bundled
    else:
        found = shutil.which(name)
        candidate = Path(found) if found else bundled
    if not candidate.exists() or not os.access(candidate, os.X_OK):
        raise FileNotFoundError(f"{name} wurde nicht gefunden: {candidate}")
    return candidate.resolve()


def run_transcription(args: argparse.Namespace) -> int:
    if not args.file:
        raise ValueError("Mindestens eine --file-Angabe ist erforderlich.")
    script_root = Path(__file__).resolve().parent
    resource_root = Path(args.resource_root).resolve() if args.resource_root else script_root
    whisper_cli = resolve_executable(args.whisper_cli, resource_root / "bin" / "whisper-cli", "whisper-cli")
    ffmpeg = resolve_executable(args.ffmpeg, resource_root / "bin" / "ffmpeg", "ffmpeg")
    models_dir = Path(args.models_dir).resolve() if args.models_dir else resource_root / "models"
    speaker_models = Path(args.speaker_models_dir).resolve() if args.speaker_models_dir else models_dir / "speaker"
    model_path = models_dir / MODEL_FILES[args.model]
    if not model_path.exists():
        raise FileNotFoundError(f"Whisper-Modell fehlt: {model_path}")

    state_dir = Path(args.state_dir).expanduser().resolve() if args.state_dir else Path.home() / "Library" / "Application Support" / "Transcription macOS" / "Jobs"
    state_dir.mkdir(parents=True, exist_ok=True)
    formats = set(args.formats or ["markdown"])
    minimum, maximum = parse_speaker_range(args.speaker_range)
    succeeded = 0
    source_metadata = {}
    if args.source_metadata_file:
        metadata_path = Path(args.source_metadata_file).expanduser().resolve()
        try:
            loaded = json.loads(metadata_path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as error:
            raise ValueError(f"Podcast-Metadaten konnten nicht gelesen werden: {error}") from error
        if not isinstance(loaded, dict):
            raise ValueError("Podcast-Metadaten müssen nach Quellpfad geordnet sein.")
        source_metadata = loaded
    emit("started", file_count=len(args.file), model=args.model, language=args.language)

    for file_index, raw_path in enumerate(args.file):
        source = Path(raw_path).expanduser().resolve()
        file_id = f"file-{file_index}"
        if not source.is_file():
            emit("file_error", file_id=file_id, source_path=str(source), message="Quelldatei wurde nicht gefunden.")
            continue
        try:
            emit("progress", file_id=file_id, source_path=str(source), stage="normalize", percent=2, message="Bereite Audio vor …")
            with tempfile.TemporaryDirectory(prefix="transcription-macos-") as temporary:
                work = Path(temporary)
                wav_path = work / "audio.wav"
                normalize_audio(source, wav_path, ffmpeg)

                regions = []
                if args.diarize:
                    emit("progress", file_id=file_id, source_path=str(source), stage="diarize", percent=10, message="Erkenne Sprecher …")
                    regions = diarize_audio(
                        wav_path,
                        speaker_models,
                        min_speakers=minimum,
                        max_speakers=maximum,
                        threshold=args.cluster_threshold,
                    )
                    emit(
                        "progress",
                        file_id=file_id,
                        source_path=str(source),
                        stage="diarize",
                        percent=35,
                        message=f"{len(set(region.speaker for region in regions))} Sprecher erkannt.",
                    )

                start_percent = 35 if args.diarize else 10

                def on_whisper_progress(percent: int, message: str) -> None:
                    overall = start_percent + int(percent * (80 - start_percent) / 100)
                    emit("progress", file_id=file_id, source_path=str(source), stage="transcribe", percent=overall, message=message)

                tokens = transcribe_words(
                    wav_path,
                    whisper_cli,
                    model_path,
                    args.language,
                    work / "whisper",
                    on_whisper_progress,
                    args.prompt,
                )
                emit("progress", file_id=file_id, source_path=str(source), stage="merge", percent=88, message="Führe Text und Sprecher zusammen …")
                document = build_document(
                    source,
                    args.language,
                    args.model,
                    tokens,
                    regions,
                    args.timecodes,
                    podcast=source_metadata.get(str(source)) or source_metadata.get(str(Path(raw_path).expanduser())),
                )
                target = Path(args.output_dir).expanduser().resolve() if args.output_dir else source.parent
                outputs = render_outputs(document, target, formats)
                document_path = write_document(document, state_dir)
                succeeded += 1
                emit(
                    "result",
                    file_id=file_id,
                    source_path=str(source),
                    document_path=str(document_path),
                    outputs=outputs,
                    speaker_count=len(document.speaker_names),
                    segment_count=len(document.segments),
                )
        except KeyboardInterrupt:
            emit("cancelled", file_id=file_id, source_path=str(source))
            return 130
        except Exception as error:
            emit("file_error", file_id=file_id, source_path=str(source), message=str(error))

    emit("batch_complete", succeeded=succeeded, failed=len(args.file) - succeeded)
    return 0 if succeeded else 1


def run_render(args: argparse.Namespace) -> int:
    if not args.document:
        raise ValueError("--document ist für den Render-Modus erforderlich.")
    path = Path(args.document).resolve()
    document = read_document(path)
    replacements = {}
    for item in args.replace:
        if "=" not in item:
            raise ValueError(f"Ungültige Ersetzung: {item}")
        source, destination = item.split("=", 1)
        replacements[source] = destination
    apply_replacements(document, replacements)
    if args.output_dir:
        formats = set(args.formats or document.outputs or ["markdown"])
        outputs = render_outputs(document, Path(args.output_dir).resolve(), formats)
    else:
        outputs = render_existing_outputs(document, set(args.formats) if args.formats else None)
    write_document(document, path.parent)
    emit("result", document_path=str(path), outputs=outputs, speaker_count=len(document.speaker_names), segment_count=len(document.segments))
    return 0


def run_glossary(args: argparse.Namespace) -> int:
    if not args.document:
        raise ValueError("--document ist für die Begriffsprüfung erforderlich.")
    document = read_document(Path(args.document).resolve())
    text = " ".join(segment.text for segment in document.segments)
    emit("glossary", candidates=extract_candidates(text, document.language))
    return 0


def main() -> int:
    args = parse_args()
    try:
        if args.mode == "render":
            return run_render(args)
        if args.mode == "glossary":
            return run_glossary(args)
        return run_transcription(args)
    except KeyboardInterrupt:
        emit("cancelled")
        return 130
    except Exception as error:
        emit("fatal_error", message=str(error))
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
