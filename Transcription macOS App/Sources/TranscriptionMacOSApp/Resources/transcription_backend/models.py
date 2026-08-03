from __future__ import annotations

from dataclasses import asdict, dataclass, field
from pathlib import Path
from typing import Any


@dataclass
class WordToken:
    start: float
    end: float
    text: str


@dataclass
class SpeakerRegion:
    start: float
    end: float
    speaker: str


@dataclass
class TranscriptSegment:
    id: str
    start: float
    end: float
    speaker: str | None
    text: str


@dataclass
class TranscriptDocument:
    id: str
    source_path: str
    source_file: str
    language: str
    model: str
    device: str = "metal"
    engine: str = "whisper.cpp"
    created: str = ""
    fps_timecode: int = 25
    timecodes: bool = True
    diarization: bool = False
    speaker_model: str | None = None
    speaker_names: dict[str, str] = field(default_factory=dict)
    speaker_regions: list[SpeakerRegion] = field(default_factory=list)
    segments: list[TranscriptSegment] = field(default_factory=list)
    outputs: dict[str, str] = field(default_factory=dict)

    def to_dict(self) -> dict[str, Any]:
        return asdict(self)

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> "TranscriptDocument":
        data = dict(data)
        data["speaker_regions"] = [SpeakerRegion(**item) for item in data.get("speaker_regions", [])]
        data["segments"] = [TranscriptSegment(**item) for item in data.get("segments", [])]
        return cls(**data)

    @property
    def source(self) -> Path:
        return Path(self.source_path)
