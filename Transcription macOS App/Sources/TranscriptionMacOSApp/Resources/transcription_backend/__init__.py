"""Local, offline transcription backend used by the native macOS app."""

from .models import SpeakerRegion, TranscriptDocument, TranscriptSegment, WordToken

__all__ = ["SpeakerRegion", "TranscriptDocument", "TranscriptSegment", "WordToken"]
