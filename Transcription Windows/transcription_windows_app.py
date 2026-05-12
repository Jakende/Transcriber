from __future__ import annotations

import os
import queue
import shutil
import sys
import threading
import time
import traceback
from datetime import datetime
from pathlib import Path
from tkinter import BOTH, END, LEFT, RIGHT, X, BooleanVar, IntVar, StringVar, Tk, filedialog, messagebox
from tkinter import ttk

try:
    import torch
    import whisper
    DEPENDENCY_ERROR = None
except ModuleNotFoundError as exc:
    torch = None
    whisper = None
    DEPENDENCY_ERROR = exc


APP_TITLE = "Transcription Windows"
FPS = 25
SUPPORTED_FILETYPES = [
    (
        "Audio and Video Files",
        "*.wav *.mp3 *.m4a *.flac *.aac *.ogg *.wma *.mp4 *.mov *.m4v *.avi *.mkv *.webm",
    ),
    ("Audio Files", "*.wav *.mp3 *.m4a *.flac *.aac *.ogg *.wma"),
    ("Video Files", "*.mp4 *.mov *.m4v *.avi *.mkv *.webm"),
    ("All Files", "*.*"),
]


def bundled_resource_path(name: str) -> Path:
    if getattr(sys, "frozen", False) and hasattr(sys, "_MEIPASS"):
        return Path(sys._MEIPASS) / name
    return Path(__file__).resolve().parent / name


def configure_bundled_ffmpeg() -> Path | None:
    ffmpeg_exe = bundled_resource_path("ffmpeg.exe")
    if ffmpeg_exe.exists():
        os.environ["PATH"] = f"{ffmpeg_exe.parent}{os.pathsep}{os.environ.get('PATH', '')}"
        return ffmpeg_exe
    return None


BUNDLED_FFMPEG = configure_bundled_ffmpeg()


def seconds_to_timecode(seconds: float, fps: int = FPS) -> str:
    total_frames = int(round(max(0.0, seconds) * fps))
    frames_per_hour = 3600 * fps
    frames_per_minute = 60 * fps

    hours = total_frames // frames_per_hour
    total_frames %= frames_per_hour
    minutes = total_frames // frames_per_minute
    total_frames %= frames_per_minute
    secs = total_frames // fps
    frames = total_frames % fps

    return f"{hours:02d}:{minutes:02d}:{secs:02d}:{frames:02d}"


def yaml_quote(value: str) -> str:
    return '"' + value.replace("\\", "\\\\").replace('"', '\\"') + '"'


def densify_sparse_buffers(module: torch.nn.Module) -> None:
    if torch is None:
        raise RuntimeError("Python package 'torch' is not installed.")
    for key, buf in list(module._buffers.items()):
        if torch.is_tensor(buf) and getattr(buf, "is_sparse", False):
            module._buffers[key] = buf.to_dense()
    for child in module.children():
        densify_sparse_buffers(child)


def pick_preferred_device() -> str:
    if torch is None:
        return "cpu"
    if torch.cuda.is_available():
        return "cuda"
    return "cpu"


def load_whisper_model_robust(model_name: str, preferred_device: str) -> tuple[torch.nn.Module, str]:
    if torch is None or whisper is None:
        raise RuntimeError("Python packages 'torch' and 'openai-whisper' are required.")
    model = whisper.load_model(model_name, device="cpu")
    densify_sparse_buffers(model)

    if preferred_device == "cuda":
        try:
            model = model.to("cuda")
            return model, "cuda"
        except Exception:
            return model, "cpu"

    return model, "cpu"


def write_markdown_transcript(
    media_path: Path,
    output_dir: Path,
    model_name: str,
    device: str,
    result: dict,
    include_timecodes: bool,
) -> Path:
    media_name = media_path.name
    media_stem = media_path.stem
    output_path = output_dir / f"{media_stem}.md"

    segments = result.get("segments", [])
    full_text = (result.get("text") or "").strip()

    segment_blocks = []
    for seg in segments:
        start_tc = seconds_to_timecode(float(seg.get("start", 0.0)))
        end_tc = seconds_to_timecode(float(seg.get("end", 0.0)))
        seg_text = (seg.get("text") or "").strip()
        if seg_text:
            segment_blocks.append(f"{start_tc} - {end_tc}\n{seg_text}")

    timestamp = datetime.now().strftime("%Y-%m-%d %H:%M")
    if include_timecodes:
        transcript_body = "\n\n".join(segment_blocks).strip() if segment_blocks else full_text
    else:
        transcript_body = full_text

    markdown_content = f"""\
---
created: {yaml_quote(timestamp)}
model: {yaml_quote(model_name)}
device: {yaml_quote(device)}
source_file: {yaml_quote(media_name)}
fps_timecode: {FPS}
timecodes: {str(include_timecodes).lower()}
---

# Transkript: {media_stem}

---

{transcript_body}
"""

    output_path.write_text(markdown_content, encoding="utf-8")
    return output_path


class TranscriptionApp:
    def __init__(self, root: Tk) -> None:
        self.root = root
        self.root.title(APP_TITLE)
        self.root.geometry("860x620")
        self.root.minsize(760, 540)

        self.files: list[Path] = []
        self.log_queue: queue.Queue[tuple[str, str | None]] = queue.Queue()
        self.worker: threading.Thread | None = None

        self.language = StringVar(value="de")
        self.model_name = StringVar(value="large")
        self.output_mode_same_folder = BooleanVar(value=True)
        self.output_dir = StringVar(value="")
        self.buffer_seconds = IntVar(value=2)
        self.include_timecodes = BooleanVar(value=True)
        self.status = StringVar(value="Bereit")

        self._build_ui()
        self._check_environment()
        self._poll_log_queue()

    def _build_ui(self) -> None:
        style = ttk.Style()
        style.theme_use("clam")

        outer = ttk.Frame(self.root, padding=16)
        outer.pack(fill=BOTH, expand=True)

        title = ttk.Label(outer, text=APP_TITLE, font=("Segoe UI", 18, "bold"))
        title.pack(anchor="w")

        subtitle = ttk.Label(
            outer,
            text="Bulk-Transkription fuer Audio- und Videodateien mit OpenAI Whisper.",
            font=("Segoe UI", 10),
        )
        subtitle.pack(anchor="w", pady=(2, 14))

        controls = ttk.Frame(outer)
        controls.pack(fill=X)

        ttk.Button(controls, text="Dateien auswaehlen", command=self.select_files).pack(side=LEFT)
        ttk.Button(controls, text="Liste leeren", command=self.clear_files).pack(side=LEFT, padx=(8, 0))
        self.start_button = ttk.Button(controls, text="Transkription starten", command=self.start_transcription)
        self.start_button.pack(side=RIGHT)

        file_frame = ttk.LabelFrame(outer, text="Ausgewaehlte Dateien", padding=10)
        file_frame.pack(fill=BOTH, expand=True, pady=(14, 10))

        self.file_list = ttk.Treeview(file_frame, columns=("path",), show="tree", height=8)
        self.file_list.heading("#0", text="Datei")
        self.file_list.pack(side=LEFT, fill=BOTH, expand=True)

        file_scroll = ttk.Scrollbar(file_frame, orient="vertical", command=self.file_list.yview)
        file_scroll.pack(side=RIGHT, fill="y")
        self.file_list.configure(yscrollcommand=file_scroll.set)

        options = ttk.LabelFrame(outer, text="Einstellungen", padding=10)
        options.pack(fill=X, pady=(0, 10))

        ttk.Label(options, text="Sprache").grid(row=0, column=0, sticky="w")
        language_box = ttk.Combobox(
            options,
            textvariable=self.language,
            values=("de", "en"),
            state="readonly",
            width=12,
        )
        language_box.grid(row=1, column=0, sticky="w", padx=(0, 14), pady=(3, 0))

        ttk.Label(options, text="Whisper/Wispr Modell").grid(row=0, column=1, sticky="w")
        model_box = ttk.Combobox(
            options,
            textvariable=self.model_name,
            values=("tiny", "base", "small", "medium", "large"),
            width=18,
        )
        model_box.grid(row=1, column=1, sticky="w", padx=(0, 14), pady=(3, 0))

        ttk.Label(options, text="Buffer zwischen Dateien (Sek.)").grid(row=0, column=2, sticky="w")
        buffer_spin = ttk.Spinbox(options, from_=0, to=60, textvariable=self.buffer_seconds, width=10)
        buffer_spin.grid(row=1, column=2, sticky="w", padx=(0, 14), pady=(3, 0))

        ttk.Checkbutton(
            options,
            text="Timecodes schreiben",
            variable=self.include_timecodes,
        ).grid(row=1, column=3, sticky="w", pady=(3, 0))

        output_frame = ttk.Frame(options)
        output_frame.grid(row=2, column=0, columnspan=4, sticky="ew", pady=(12, 0))
        output_frame.columnconfigure(1, weight=1)

        ttk.Checkbutton(
            output_frame,
            text="Markdown neben Quelldatei speichern",
            variable=self.output_mode_same_folder,
            command=self._toggle_output_picker,
        ).grid(row=0, column=0, sticky="w")

        self.output_entry = ttk.Entry(output_frame, textvariable=self.output_dir, state="disabled")
        self.output_entry.grid(row=0, column=1, sticky="ew", padx=(12, 8))
        self.output_button = ttk.Button(
            output_frame,
            text="Zielordner",
            command=self.select_output_dir,
            state="disabled",
        )
        self.output_button.grid(row=0, column=2, sticky="e")

        log_frame = ttk.LabelFrame(outer, text="Protokoll", padding=10)
        log_frame.pack(fill=BOTH, expand=True)

        self.log_text = ttk.Treeview(log_frame, columns=("message",), show="tree", height=8)
        self.log_text.heading("#0", text="Status")
        self.log_text.pack(side=LEFT, fill=BOTH, expand=True)

        log_scroll = ttk.Scrollbar(log_frame, orient="vertical", command=self.log_text.yview)
        log_scroll.pack(side=RIGHT, fill="y")
        self.log_text.configure(yscrollcommand=log_scroll.set)

        status_bar = ttk.Label(outer, textvariable=self.status, anchor="w")
        status_bar.pack(fill=X, pady=(8, 0))

    def _check_environment(self) -> None:
        if DEPENDENCY_ERROR is not None:
            install_hint = (
                "Die gebaute EXE enthaelt diese Pakete normalerweise. "
                "Bitte build_windows_exe.bat erneut ausfuehren."
                if getattr(sys, "frozen", False)
                else "Bitte install_windows.bat ausfuehren oder pip install torch openai-whisper installieren."
            )
            self._log(
                "FEHLENDE DEPENDENCY: "
                f"{DEPENDENCY_ERROR}. {install_hint}"
            )
        if shutil.which("ffmpeg") is None:
            self._log("WARNUNG: ffmpeg wurde nicht auf PATH gefunden. Medien-Decoding kann fehlschlagen.")
        elif BUNDLED_FFMPEG is not None:
            self._log(f"Gebuendeltes ffmpeg aktiv: {BUNDLED_FFMPEG}")
        else:
            self._log(f"System-ffmpeg aktiv: {shutil.which('ffmpeg')}")
        self._log(f"Device-Erkennung: bevorzugt '{pick_preferred_device()}'.")

    def _toggle_output_picker(self) -> None:
        state = "disabled" if self.output_mode_same_folder.get() else "normal"
        self.output_entry.configure(state=state)
        self.output_button.configure(state=state)

    def select_files(self) -> None:
        selected = filedialog.askopenfilenames(title="Dateien auswaehlen", filetypes=SUPPORTED_FILETYPES)
        if not selected:
            return
        known = {path.resolve() for path in self.files}
        for file_name in selected:
            path = Path(file_name).resolve()
            if path not in known:
                self.files.append(path)
                known.add(path)
                self.file_list.insert("", END, text=str(path))
        self.status.set(f"{len(self.files)} Datei(en) ausgewaehlt")

    def clear_files(self) -> None:
        self.files.clear()
        for item in self.file_list.get_children():
            self.file_list.delete(item)
        self.status.set("Dateiliste geleert")

    def select_output_dir(self) -> None:
        selected = filedialog.askdirectory(title="Zielordner auswaehlen")
        if selected:
            self.output_dir.set(selected)

    def start_transcription(self) -> None:
        if self.worker and self.worker.is_alive():
            return
        if not self.files:
            messagebox.showinfo(APP_TITLE, "Bitte zuerst eine oder mehrere Dateien auswaehlen.")
            return
        if not self.output_mode_same_folder.get() and not self.output_dir.get().strip():
            messagebox.showinfo(APP_TITLE, "Bitte einen Zielordner auswaehlen.")
            return

        self.start_button.configure(state="disabled")
        self.status.set("Transkription laeuft")
        self.worker = threading.Thread(target=self._run_transcription, daemon=True)
        self.worker.start()

    def _run_transcription(self) -> None:
        model_name = self.model_name.get().strip() or "large"
        language = self.language.get().strip() or "de"
        buffer_seconds = max(0, int(self.buffer_seconds.get()))
        include_timecodes = self.include_timecodes.get()

        try:
            if DEPENDENCY_ERROR is not None:
                install_hint = (
                    "Die gebaute EXE enthaelt diese Pakete normalerweise. "
                    "Bitte build_windows_exe.bat erneut ausfuehren."
                    if getattr(sys, "frozen", False)
                    else "Bitte install_windows.bat ausfuehren oder pip install torch openai-whisper installieren."
                )
                raise RuntimeError(
                    f"{DEPENDENCY_ERROR}. {install_hint}"
                )

            preferred_device = pick_preferred_device()
            self._queue_log(f"Lade Modell '{model_name}' auf bevorzugtem device '{preferred_device}' ...")
            model, device = load_whisper_model_robust(model_name, preferred_device)
            self._queue_log(f"Modell geladen. Verwendetes device: '{device}'.")

            if device == "cpu":
                try:
                    torch.set_num_threads(max(1, os.cpu_count() or 1))
                except Exception:
                    pass

            for index, media_path in enumerate(self.files, start=1):
                self._queue_status(f"Transkribiere {index}/{len(self.files)}: {media_path.name}")
                self._queue_log(f"Starte {index}/{len(self.files)}: {media_path.name}")

                result = model.transcribe(
                    str(media_path),
                    language=language,
                    verbose=False,
                    fp16=False,
                    beam_size=1,
                    best_of=1,
                    temperature=0,
                    condition_on_previous_text=False,
                )

                output_dir = (
                    media_path.parent
                    if self.output_mode_same_folder.get()
                    else Path(self.output_dir.get()).resolve()
                )
                output_dir.mkdir(parents=True, exist_ok=True)
                output_path = write_markdown_transcript(
                    media_path,
                    output_dir,
                    model_name,
                    device,
                    result,
                    include_timecodes,
                )
                self._queue_log(f"Gespeichert: {output_path}")

                if index < len(self.files) and buffer_seconds > 0:
                    self._queue_log(f"Warte {buffer_seconds} Sekunden vor der naechsten Datei ...")
                    time.sleep(buffer_seconds)

            self._queue_status("Fertig")
            self._queue_log("Bulk-Transkription abgeschlossen.")
        except Exception as exc:
            self._queue_status("Fehler")
            self._queue_log(f"FEHLER: {exc}\n{traceback.format_exc()}")
        finally:
            self.log_queue.put(("done", None))

    def _queue_log(self, message: str) -> None:
        self.log_queue.put(("log", message))

    def _queue_status(self, message: str) -> None:
        self.log_queue.put(("status", message))

    def _poll_log_queue(self) -> None:
        while True:
            try:
                kind, message = self.log_queue.get_nowait()
            except queue.Empty:
                break

            if kind == "log" and message is not None:
                self._log(message)
            elif kind == "status" and message is not None:
                self.status.set(message)
            elif kind == "done":
                self.start_button.configure(state="normal")

        self.root.after(150, self._poll_log_queue)

    def _log(self, message: str) -> None:
        timestamp = datetime.now().strftime("%H:%M:%S")
        self.log_text.insert("", END, text=f"[{timestamp}] {message}")
        children = self.log_text.get_children()
        if children:
            self.log_text.see(children[-1])


def main() -> None:
    root = Tk()
    app = TranscriptionApp(root)
    root.mainloop()


if __name__ == "__main__":
    main()
