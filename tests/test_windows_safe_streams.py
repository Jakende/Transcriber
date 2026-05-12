import importlib.util
import sys
import unittest
from pathlib import Path
from unittest import mock


class WindowsSafeStreamsTest(unittest.TestCase):
    def test_windows_app_installs_safe_streams_when_windowed(self):
        module_path = Path(__file__).resolve().parents[1] / "Transcription Windows" / "transcription_windows_app.py"
        original_stdout = sys.stdout
        original_stderr = sys.stderr

        try:
            with mock.patch.object(sys, "stdout", None), mock.patch.object(sys, "stderr", None):
                spec = importlib.util.spec_from_file_location("transcription_windows_app_safe_stream_test", module_path)
                module = importlib.util.module_from_spec(spec)
                self.assertIsNotNone(spec.loader)
                try:
                    spec.loader.exec_module(module)
                except ModuleNotFoundError as exc:
                    if exc.name == "_tkinter":
                        self.skipTest("Local Python is not built with tkinter.")
                    raise

                self.assertIsNotNone(sys.stdout)
                self.assertIsNotNone(sys.stderr)
                self.assertEqual(sys.stderr.write("progress"), len("progress"))
        finally:
            sys.stdout = original_stdout
            sys.stderr = original_stderr


if __name__ == "__main__":
    unittest.main()
