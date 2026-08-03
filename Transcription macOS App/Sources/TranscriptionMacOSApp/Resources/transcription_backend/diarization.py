"""Offline speaker diarization adapted from LocalTranscript (MIT).

Pipeline: Silero VAD -> overlapping windows -> SpeechBrain ECAPA embeddings ->
cosine clustering. The transcription app keeps this module independent from
Whisper so the full transcript can retain its linguistic context.
"""

from __future__ import annotations

from collections import Counter
from pathlib import Path

import numpy as np
import soundfile as sf
import torch

from .models import SpeakerRegion


_vad_model = None
_embedding_model = None


def _load_audio_16k(path: Path) -> tuple[torch.Tensor, int]:
    data, sample_rate = sf.read(path, always_2d=True, dtype="float32")
    waveform = torch.from_numpy(data.T)
    if waveform.shape[0] > 1:
        waveform = waveform.mean(dim=0, keepdim=True)
    if sample_rate != 16000:
        raise ValueError("Die Diarisierung erwartet normalisiertes 16-kHz-Audio.")
    return waveform, sample_rate


def _get_vad():
    global _vad_model
    if _vad_model is None:
        from silero_vad import load_silero_vad

        _vad_model = load_silero_vad()
    return _vad_model


def _get_embedder(model_root: Path):
    global _embedding_model
    if _embedding_model is None:
        from speechbrain.inference.speaker import EncoderClassifier

        local_model = model_root / "spkrec-ecapa-voxceleb"
        source = str(local_model) if (local_model / "hyperparams.yaml").exists() else "speechbrain/spkrec-ecapa-voxceleb"
        _embedding_model = EncoderClassifier.from_hparams(
            source=source,
            savedir=str(local_model),
            run_opts={"device": "cpu"},
        )
    return _embedding_model


def _speech_regions(waveform: torch.Tensor) -> list[tuple[float, float]]:
    from silero_vad import get_speech_timestamps

    timestamps = get_speech_timestamps(
        waveform.squeeze(0).float(),
        _get_vad(),
        sampling_rate=16000,
        return_seconds=True,
        min_silence_duration_ms=150,
        min_speech_duration_ms=250,
    )
    return [(float(item["start"]), float(item["end"])) for item in timestamps]


def _windows(start: float, end: float, size: float = 1.5, hop: float = 0.75) -> list[tuple[float, float]]:
    duration = end - start
    if duration < 0.4:
        return []
    if duration <= size:
        return [(start, end)]
    result: list[tuple[float, float]] = []
    current = start
    while current + size <= end + 1e-3:
        result.append((current, current + size))
        current += hop
    if result and result[-1][1] < end - 0.15:
        result.append((max(start, end - size), end))
    return result


def _embed(waveform: torch.Tensor, sample_rate: int, windows: list[tuple[float, float]], model_root: Path) -> np.ndarray:
    encoder = _get_embedder(model_root)
    embeddings = []
    with torch.no_grad():
        for start, end in windows:
            chunk = waveform[:, max(0, int(start * sample_rate)):min(waveform.shape[1], int(end * sample_rate))]
            minimum = int(0.2 * sample_rate)
            if chunk.shape[1] < minimum:
                chunk = torch.nn.functional.pad(chunk, (0, minimum - chunk.shape[1]))
            embeddings.append(encoder.encode_batch(chunk).squeeze().detach().cpu().numpy())
    return np.asarray(embeddings)


def _cluster(embeddings: np.ndarray, fixed_speakers: int | None, threshold: float) -> np.ndarray:
    from sklearn.cluster import AgglomerativeClustering, SpectralClustering

    if len(embeddings) == 1:
        return np.zeros(1, dtype=int)
    normalized = embeddings / (np.linalg.norm(embeddings, axis=1, keepdims=True) + 1e-9)
    affinity = np.clip(normalized @ normalized.T, 0.0, 1.0)
    distance = 1.0 - affinity
    np.fill_diagonal(distance, 0.0)

    if fixed_speakers and 1 < fixed_speakers < len(embeddings):
        return SpectralClustering(
            n_clusters=fixed_speakers,
            affinity="precomputed",
            assign_labels="kmeans",
            random_state=0,
        ).fit_predict(affinity)

    labels = AgglomerativeClustering(
        n_clusters=None,
        metric="precomputed",
        linkage="average",
        distance_threshold=0.5 + max(0.0, min(0.85, threshold)) * 0.55,
    ).fit_predict(distance)

    for _ in range(2):
        unique = np.unique(labels)
        centroids = []
        for label in unique:
            mean = normalized[labels == label].mean(axis=0)
            centroids.append(mean / (np.linalg.norm(mean) + 1e-9))
        refined = unique[np.argmax(normalized @ np.asarray(centroids).T, axis=1)]
        if np.array_equal(refined, labels):
            break
        labels = refined
    return labels


def diarize_audio(
    wav_path: Path,
    model_root: Path,
    min_speakers: int = 0,
    max_speakers: int = 0,
    threshold: float = 0.5,
) -> list[SpeakerRegion]:
    waveform, sample_rate = _load_audio_16k(wav_path)
    speech_regions = _speech_regions(waveform)
    if not speech_regions:
        return []

    windows: list[tuple[float, float]] = []
    region_indexes: list[int] = []
    for index, (start, end) in enumerate(speech_regions):
        for window in _windows(start, end):
            windows.append(window)
            region_indexes.append(index)
    if not windows:
        return []

    embeddings = _embed(waveform, sample_rate, windows, model_root)
    fixed = min_speakers if min_speakers > 0 and min_speakers == max_speakers else None
    labels = _cluster(embeddings, fixed, threshold)
    found = len(set(int(label) for label in labels))
    if max_speakers > 0 and found > max_speakers:
        labels = _cluster(embeddings, max_speakers, threshold)
    elif min_speakers > 0 and found < min_speakers and len(embeddings) >= min_speakers:
        labels = _cluster(embeddings, min_speakers, threshold)

    labels_by_region: dict[int, list[int]] = {}
    for index, label in zip(region_indexes, labels):
        labels_by_region.setdefault(index, []).append(int(label))

    regions: list[SpeakerRegion] = []
    for index, (start, end) in enumerate(speech_regions):
        candidates = labels_by_region.get(index, [])
        if candidates:
            label = Counter(candidates).most_common(1)[0][0]
            regions.append(SpeakerRegion(start, end, f"SPEAKER_{label:02d}"))

    totals: dict[str, float] = {}
    for region in regions:
        totals[region.speaker] = totals.get(region.speaker, 0.0) + region.end - region.start
    reliable = {speaker for speaker, duration in totals.items() if duration >= 2.0}
    if reliable:
        for region in regions:
            if region.speaker not in reliable:
                midpoint = (region.start + region.end) / 2
                nearest = min(
                    (other for other in regions if other.speaker in reliable),
                    key=lambda other: abs((other.start + other.end) / 2 - midpoint),
                )
                region.speaker = nearest.speaker

    merged: list[SpeakerRegion] = []
    for region in sorted(regions, key=lambda item: item.start):
        if merged and merged[-1].speaker == region.speaker and region.start - merged[-1].end < 0.5:
            merged[-1].end = region.end
        else:
            merged.append(region)

    durations: dict[str, float] = {}
    for region in merged:
        durations[region.speaker] = durations.get(region.speaker, 0.0) + region.end - region.start
    ordered = sorted(durations, key=lambda speaker: -durations[speaker])
    relabel = {speaker: f"SPEAKER_{index:02d}" for index, speaker in enumerate(ordered)}
    for region in merged:
        region.speaker = relabel[region.speaker]
    return merged


def parse_speaker_range(value: str) -> tuple[int, int]:
    if value == "auto":
        return 0, 0
    try:
        minimum, maximum = value.split("-", 1)
        return max(0, int(minimum)), max(0, int(maximum))
    except (ValueError, TypeError):
        raise ValueError(f"Ungültiger Sprecherbereich: {value}") from None
