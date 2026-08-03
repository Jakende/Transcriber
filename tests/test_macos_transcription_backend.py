import json
import sys
import tempfile
import unittest
from pathlib import Path


BACKEND_ROOT = (
    Path(__file__).resolve().parents[1]
    / "Transcription macOS App"
    / "Sources"
    / "TranscriptionMacOSApp"
    / "Resources"
)
sys.path.insert(0, str(BACKEND_ROOT))

from transcription_backend.document import (  # noqa: E402
    apply_replacements,
    build_document,
    fps_timecode,
    render_csv,
    render_markdown,
    render_outputs,
    render_vtt,
    segments_from_tokens,
)
from transcription_backend.media import coalesce_word_tokens, normalization_error, parse_timestamp  # noqa: E402
from transcription_backend.glossary import iter_text_chunks, merge_candidate_counts  # noqa: E402
from transcription_backend.models import SpeakerRegion, WordToken  # noqa: E402
from collections import Counter


class TranscriptionDocumentTests(unittest.TestCase):
    def test_timecodes(self):
        self.assertEqual(parse_timestamp("01:02:03,500"), 3723.5)
        self.assertEqual(fps_timecode(1.04, 25), "00:00:01:01")

    def test_video_without_audio_gets_a_clear_error(self):
        detail = "Stream map '0:a:0' matches no streams."
        self.assertEqual(normalization_error(detail), "Die Datei enthält keine Audiospur, die transkribiert werden kann.")

    def test_each_token_is_assigned_once_across_speaker_boundary(self):
        tokens = [
            WordToken(0.0, 0.5, " Hallo"),
            WordToken(0.5, 1.0, " Welt."),
            WordToken(1.1, 1.5, " Guten"),
            WordToken(1.5, 2.0, " Tag."),
        ]
        regions = [SpeakerRegion(0, 1, "SPEAKER_00"), SpeakerRegion(1.1, 2, "SPEAKER_01")]
        segments = segments_from_tokens(tokens, regions)
        self.assertEqual([segment.speaker for segment in segments], ["SPEAKER_00", "SPEAKER_01"])
        self.assertEqual(" ".join(segment.text for segment in segments), "Hallo Welt. Guten Tag.")

    def test_trailing_punctuation_does_not_create_long_empty_segment(self):
        tokens = [WordToken(0, 4.35, " Guten Tag"), WordToken(4.35, 30, ".")]
        segments = segments_from_tokens(tokens, [])
        self.assertEqual(len(segments), 1)
        self.assertEqual(segments[0].text, "Guten Tag.")
        self.assertEqual(segments[0].end, 4.35)

    def test_whisper_subword_tokens_are_coalesced_before_diarization(self):
        raw = [
            {"text": " wicht", "timestamps": {"from": "00:00:01,000", "to": "00:00:01,300"}},
            {"text": "igen", "timestamps": {"from": "00:00:01,300", "to": "00:00:01,500"}},
            {"text": " Aufgaben", "timestamps": {"from": "00:00:01,500", "to": "00:00:02,000"}},
            {"text": ".", "timestamps": {"from": "00:00:30,000", "to": "00:00:30,000"}},
            {"text": "[_EOT_]", "timestamps": {"from": "00:00:30,000", "to": "00:00:30,000"}},
        ]
        words = coalesce_word_tokens(raw)
        self.assertEqual([(word.text, word.start, word.end) for word in words], [
            (" wichtigen", 1.0, 1.5),
            (" Aufgaben.", 1.5, 2.0),
        ])

    def test_exports_preserve_yaml_and_speakers(self):
        document = build_document(
            Path("/tmp/interview.wav"),
            "de",
            "turbo",
            [WordToken(0, 1, " Hallo."), WordToken(1.1, 2, " Willkommen.")],
            [SpeakerRegion(0, 1, "SPEAKER_00"), SpeakerRegion(1.1, 2, "SPEAKER_01")],
            True,
        )
        markdown = render_markdown(document)
        self.assertIn('model: "turbo"', markdown)
        self.assertIn("fps_timecode: 25", markdown)
        self.assertIn("diarization: true", markdown)
        self.assertIn("**Sprecher 1:**", markdown)
        self.assertIn("WEBVTT", render_vtt(document))
        self.assertIn("speaker", render_csv(document).splitlines()[0])

    def test_replacements_use_word_boundaries(self):
        document = build_document(Path("/tmp/a.wav"), "de", "small", [WordToken(0, 1, " Plan Planer")], [], False)
        apply_replacements(document, {"Plan": "Entwurf"})
        self.assertEqual(document.segments[0].text, "Entwurf Planer")

    def test_glossary_merges_duplicate_terms_across_entity_kinds(self):
        candidates = Counter({("Musk", "PER"): 2, ("Musk", "PROPN"): 1, ("musk", "ORG"): 3})
        merged = merge_candidate_counts(candidates)
        self.assertEqual(len(merged), 1)
        self.assertEqual(merged[0]["term"], "Musk")
        self.assertEqual(merged[0]["count"], 6)
        self.assertEqual(set(merged[0]["kind"].split(" / ")), {"PER", "PROPN", "ORG"})

    def test_glossary_chunks_long_text_without_losing_content(self):
        text = ("Ein langer Satz mit Fachbegriff. " * 8).strip()
        chunks = list(iter_text_chunks(text, max_characters=60))
        self.assertGreater(len(chunks), 1)
        self.assertEqual(" ".join(chunks), text)

    def test_output_collision_gets_suffix_and_document_is_serializable(self):
        document = build_document(Path("/tmp/a.wav"), "de", "small", [WordToken(0, 1, " Text")], [], False)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "a.md").write_text("vorhanden", encoding="utf-8")
            outputs = render_outputs(document, root, {"markdown", "vtt"})
            self.assertEqual(Path(outputs["markdown"]).name, "a_2.md")
            self.assertTrue(Path(outputs["vtt"]).exists())
            json.dumps(document.to_dict(), ensure_ascii=False)


if __name__ == "__main__":
    unittest.main()
