import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock


WINDOWS_ROOT = Path(__file__).resolve().parents[1] / "Transcription Windows"
if str(WINDOWS_ROOT) not in sys.path:
    sys.path.insert(0, str(WINDOWS_ROOT))

import windows_services as services  # noqa: E402
from transcription_backend.document import build_document  # noqa: E402
from transcription_backend.models import SpeakerRegion, WordToken  # noqa: E402


class WindowsServiceTests(unittest.TestCase):
    def test_queue_persistence_keeps_podcast_metadata(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            media = root / "episode.mp3"
            media.write_bytes(b"test")
            store = services.WindowsStore(root / "appdata")
            store.save_queue([services.QueueItem(str(media), {"show_title": "Planung"})])
            loaded = store.load_queue()
        self.assertEqual(len(loaded), 1)
        self.assertEqual(loaded[0].podcast, {"show_title": "Planung"})

    def test_vtt_import_preserves_speaker_and_audio_source(self):
        content = "WEBVTT\n\n00:00:00.000 --> 00:00:02.000\nAda: Guten Tag.\n\n00:02.000 --> 00:04.000\nLinus: Hello there.\n"
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            vtt = root / "sample.vtt"
            audio = root / "sample.wav"
            vtt.write_text(content, encoding="utf-8")
            audio.write_bytes(b"audio")
            document = services.parse_vtt(vtt, audio)
        self.assertEqual(document.source_path, str(audio.resolve()))
        self.assertEqual(document.speaker_names, {"SPEAKER_00": "Ada", "SPEAKER_01": "Linus"})
        self.assertEqual([segment.text for segment in document.segments], ["Guten Tag.", "Hello there."])

    def test_segment_editing_and_speaker_merge(self):
        document = build_document(
            Path("interview.wav"),
            "de",
            "small",
            [WordToken(0, 1, " Eins zwei drei vier")],
            [SpeakerRegion(0, 1, "SPEAKER_00")],
            True,
        )
        segment_id = document.segments[0].id
        services.split_segment(document, segment_id)
        self.assertEqual(len(document.segments), 2)
        services.merge_segment_with_next(document, segment_id)
        self.assertEqual(len(document.segments), 1)
        document.speaker_names["SPEAKER_01"] = "Gast"
        document.segments[0].speaker = "SPEAKER_01"
        services.merge_speakers(document, "SPEAKER_01", "SPEAKER_00")
        self.assertEqual(document.segments[0].speaker, "SPEAKER_00")

    def test_youtube_validation_rejects_playlists_and_non_https(self):
        self.assertEqual(
            services.validate_youtube_url("https://www.youtube.com/watch?v=abc123"),
            "https://www.youtube.com/watch?v=abc123",
        )
        with self.assertRaises(ValueError):
            services.validate_youtube_url("https://www.youtube.com/playlist?list=abc")
        with self.assertRaises(ValueError):
            services.validate_youtube_url("http://youtu.be/abc123")

    def test_rss_loader_only_exposes_episodes_with_enclosures(self):
        payload = b"""<?xml version='1.0'?>
        <rss><channel><title>Stadt und Klima</title><language>de</language>
        <item><title>Folge 1</title><guid>one</guid><enclosure url='https://cdn.example/one.mp3' type='audio/mpeg'/></item>
        <item><title>Ohne Audio</title></item>
        </channel></rss>"""
        with mock.patch.object(services, "_download_bounded", return_value=(payload, "https://example.org/feed.xml")):
            feed = services.load_rss_feed("https://example.org/feed.xml")
        self.assertEqual(feed.title, "Stadt und Klima")
        self.assertEqual(len(feed.episodes), 1)
        self.assertEqual(feed.episodes[0].id, "one")

    def test_language_annotation_handles_german_and_english(self):
        self.assertEqual(services.detect_segment_language("Das ist ein deutscher Satz und wir sind hier."), "de")
        self.assertEqual(services.detect_segment_language("This is the English sentence and we are here."), "en")


if __name__ == "__main__":
    unittest.main()
