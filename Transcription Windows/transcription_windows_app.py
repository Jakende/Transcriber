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
from tkinter import BOTH, END, LEFT, RIGHT, X, BooleanVar, IntVar, StringVar, Text, Tk, filedialog, messagebox
from tkinter import ttk


class SafeNullWriter:
    def write(self, text: str) -> int:
        return len(text or "")

    def flush(self) -> None:
        pass


if sys.stdout is None:
    sys.stdout = SafeNullWriter()
if sys.stderr is None:
    sys.stderr = SafeNullWriter()


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


def app_data_dir() -> Path:
    base = os.environ.get("LOCALAPPDATA") or os.environ.get("APPDATA") or str(Path.home())
    path = Path(base) / APP_TITLE
    path.mkdir(parents=True, exist_ok=True)
    return path


def whisper_model_cache_dir() -> Path:
    path = app_data_dir() / "models"
    path.mkdir(parents=True, exist_ok=True)
    return path


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
    model = whisper.load_model(model_name, device="cpu", download_root=str(whisper_model_cache_dir()))
    densify_sparse_buffers(model)
    if preferred_device == "cuda":
        try:
            return model.to("cuda"), "cuda"
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
    transcript_body = "\n\n".join(segment_blocks).strip() if include_timecodes and segment_blocks else full_text
    markdown_content = f"""\
---
created: {yaml_quote(timestamp)}
model: {yaml_quote(model_name)}
device: {yaml_quote(device)}
source_file: {yaml_quote(media_name)}
fps_timecode: {FPS}
timecodes: {str(include_timecodes).lower()}
---

# Transcript: {media_stem}

---

{transcript_body}
"""
    output_path.write_text(markdown_content, encoding="utf-8")
    return output_path


class TranscriptionApp:
    def __init__(self, root: Tk) -> None:
        self.root = root
        self.root.title(APP_TITLE)
        self.root.geometry("1040x720")
        self.root.minsize(900, 620)

        self.files: list[Path] = []
        self.log_queue: queue.Queue[tuple[str, str | None]] = queue.Queue()
        self.worker: threading.Thread | None = None

        self.language = StringVar(value="de")
        self.model_name = StringVar(value="large")
        self.output_mode_same_folder = BooleanVar(value=True)
        self.output_dir = StringVar(value="")
        self.buffer_seconds = IntVar(value=2)
        self.include_timecodes = BooleanVar(value=True)
        self.status = StringVar(value="Ready")
        self.progress = StringVar(value="Idle")

        self._build_ui()
        self._check_environment()
        self._poll_log_queue()

    def _build_ui(self) -> None:
        style = ttk.Style()
        if "vista" in style.theme_names():
            style.theme_use("vista")
        elif "xpnative" in style.theme_names():
            style.theme_use("xpnative")

        style.configure("Title.TLabel", font=("Segoe UI", 22, "bold"))
        style.configure("Subtitle.TLabel", font=("Segoe UI", 10), foreground="#555555")
        style.configure("Section.TLabelframe.Label", font=("Segoe UI", 10, "bold"))
        style.configure("Primary.TButton", font=("Segoe UI", 10, "bold"))

        outer = ttk.Frame(self.root, padding=22)
        outer.pack(fill=BOTH, expand=True)
        outer.columnconfigure(0, weight=1)
        outer.rowconfigure(2, weight=1)
        outer.rowconfigure(4, weight=1)

        header = ttk.Frame(outer)
        header.grid(row=0, column=0, sticky="ew")
        header.columnconfigure(0, weight=1)
        ttk.Label(header, text=APP_TITLE, style="Title.TLabel").grid(row=0, column=0, sticky="w")
        ttk.Label(
            header,
            text="Bulk transcribe local audio and video files with Whisper.",
            style="Subtitle.TLabel",
        ).grid(row=1, column=0, sticky="w", pady=(4, 0))

        actions = ttk.Frame(header)
        actions.grid(row=0, column=1, rowspan=2, sticky="e")
        ttk.Button(actions, text="Add Files", command=self.select_files).pack(side=LEFT, padx=(0, 8))
        ttk.Button(actions, text="Clear", command=self.clear_files).pack(side=LEFT)

        file_frame = ttk.LabelFrame(outer, text="Selected files", padding=10, style="Section.TLabelframe")
        file_frame.grid(row=2, column=0, sticky="nsew", pady=(18, 12))
        file_frame.columnconfigure(0, weight=1)
        file_frame.rowconfigure(0, weight=1)
        self.file_list = ttk.Treeview(file_frame, columns=("folder",), show="headings", height=7)
        self.file_list.heading("folder", text="File")
        self.file_list.column("folder", anchor="w")
        self.file_list.grid(row=0, column=0, sticky="nsew")
        file_scroll = ttk.Scrollbar(file_frame, orient="vertical", command=self.file_list.yview)
        file_scroll.grid(row=0, column=1, sticky="ns")
        self.file_list.configure(yscrollcommand=file_scroll.set)

        options = ttk.LabelFrame(outer, text="Settings", padding=12, style="Section.TLabelframe")
        options.grid(row=3, column=0, sticky="ew", pady=(0, 12))
        for col in range(5):
            options.columnconfigure(col, weight=1 if col == 4 else 0)

        ttk.Label(options, text="Language").grid(row=0, column=0, sticky="w")
        ttk.Combobox(options, textvariable=self.language, values=("de", "en"), state="readonly", width=10).grid(
            row=1, column=0, sticky="w", padx=(0, 18), pady=(4, 10)
        )

        ttk.Label(options, text="Whisper model").grid(row=0, column=1, sticky="w")
        ttk.Combobox(
            options,
            textvariable=self.model_name,
            values=("tiny", "base", "small", "medium", "large"),
            width=16,
        ).grid(row=1, column=1, sticky="w", padx=(0, 18), pady=(4, 10))

        ttk.Label(options, text="Buffer").grid(row=0, column=2, sticky="w")
        ttk.Spinbox(options, from_=0, to=60, textvariable=self.buffer_seconds, width=8).grid(
            row=1, column=2, sticky="w", padx=(0, 18), pady=(4, 10)
        )

        ttk.Checkbutton(options, text="Write timecodes", variable=self.include_timecodes).grid(
            row=1, column=3, sticky="w", pady=(4, 10)
        )

        ttk.Checkbutton(
            options,
            text="Save Markdown next to source files",
            variable=self.output_mode_same_folder,
            command=self._toggle_output_picker,
        ).grid(row=2, column=0, columnspan=2, sticky="w")

        self.output_entry = ttk.Entry(options, textvariable=self.output_dir, state="disabled")
        self.output_entry.grid(row=2, column=2, columnspan=2, sticky="ew", padx=(0, 8))
        self.output_button = ttk.Button(options, text="Choose Folder", command=self.select_output_dir, state="disabled")
        self.output_button.grid(row=2, column=4, sticky="e")

        run_bar = ttk.Frame(outer)
        run_bar.grid(row=4, column=0, sticky="ew", pady=(0, 12))
        run_bar.columnconfigure(0, weight=1)
        ttk.Label(run_bar, textvariable=self.status, font=("Segoe UI", 10, "bold")).grid(row=0, column=0, sticky="w")
        ttk.Label(run_bar, textvariable=self.progress, foreground="#666666").grid(row=1, column=0, sticky="w", pady=(2, 0))
        self.start_button = ttk.Button(
            run_bar,
            text="Start Transcription",
            command=self.start_transcription,
            style="Primary.TButton",
        )
        self.start_button.grid(row=0, column=1, rowspan=2, sticky="e")

        log_frame = ttk.LabelFrame(outer, text="Activity", padding=10, style="Section.TLabelframe")
        log_frame.grid(row=5, column=0, sticky="nsew")
        log_frame.columnconfigure(0, weight=1)
        log_frame.rowconfigure(0, weight=1)
        self.log_text = Text(
            log_frame,
            height=8,
            wrap="word",
            font=("Consolas", 9),
            borderwidth=0,
            padx=10,
            pady=8,
            state="disabled",
        )
        self.log_text.grid(row=0, column=0, sticky="nsew")
        log_scroll = ttk.Scrollbar(log_frame, orient="vertical", command=self.log_text.yview)
        log_scroll.grid(row=0, column=1, sticky="ns")
        self.log_text.configure(yscrollcommand=log_scroll.set)

    def _check_environment(self) -> None:
        if DEPENDENCY_ERROR is not None:
            install_hint = (
                "The installed EXE should include these packages. Rebuild the installer."
                if getattr(sys, "frozen", False)
                else "Run install_windows.bat or install torch and openai-whisper in this Python environment."
            )
            self._log(f"Missing dependency: {DEPENDENCY_ERROR}. {install_hint}", "error")
        if shutil.which("ffmpeg") is None:
            self._log("FFmpeg was not found. Media decoding may fail.", "error")
        elif BUNDLED_FFMPEG is not None:
            self._log("Bundled FFmpeg is active.", "success")
        else:
            self._log(f"System FFmpeg is active: {shutil.which('ffmpeg')}", "info")
        self._log(f"Preferred device: {pick_preferred_device()}.", "info")

    def _toggle_output_picker(self) -> None:
        state = "disabled" if self.output_mode_same_folder.get() else "normal"
        self.output_entry.configure(state=state)
        self.output_button.configure(state=state)

    def select_files(self) -> None:
        selected = filedialog.askopenfilenames(title="Select files", filetypes=SUPPORTED_FILETYPES)
        if not selected:
            return
        known = {path.resolve() for path in self.files}
        for file_name in selected:
            path = Path(file_name).resolve()
            if path not in known:
                self.files.append(path)
                known.add(path)
                self.file_list.insert("", END, values=(f"{path.name}    {path.parent}",))
        self.status.set(self.file_count_text())

    def clear_files(self) -> None:
        self.files.clear()
        for item in self.file_list.get_children():
            self.file_list.delete(item)
        self.status.set("No files selected")
        self.progress.set("Idle")

    def select_output_dir(self) -> None:
        selected = filedialog.askdirectory(title="Choose destination folder")
        if selected:
            self.output_dir.set(selected)

    def start_transcription(self) -> None:
        if self.worker and self.worker.is_alive():
            return
        if not self.files:
            messagebox.showinfo(APP_TITLE, "Select one or more files first.")
            return
        if not self.output_mode_same_folder.get() and not self.output_dir.get().strip():
            messagebox.showinfo(APP_TITLE, "Choose a destination folder first.")
            return
        self.start_button.configure(state="disabled")
        self.status.set("Running")
        self.progress.set("Preparing transcription...")
        self.worker = threading.Thread(target=self._run_transcription, daemon=True)
        self.worker.start()

    def _run_transcription(self) -> None:
        model_name = self.model_name.get().strip() or "large"
        language = self.language.get().strip() or "de"
        buffer_seconds = max(0, int(self.buffer_seconds.get()))
        include_timecodes = self.include_timecodes.get()
        try:
            if DEPENDENCY_ERROR is not None:
                raise RuntimeError(
                    f"{DEPENDENCY_ERROR}. The installed EXE should include torch and openai-whisper."
                )
            preferred_device = pick_preferred_device()
            self._queue_log(f"Loading model '{model_name}' on preferred device '{preferred_device}' ...", "info")
            self._queue_log(f"Model cache: {whisper_model_cache_dir()}", "info")
            self._queue_log("If this model is not cached yet, the first download can take several minutes.", "info")
            model, device = load_whisper_model_robust(model_name, preferred_device)
            self._queue_log(f"Model loaded. Using device '{device}'.", "success")
            if device == "cpu":
                try:
                    torch.set_num_threads(max(1, os.cpu_count() or 1))
                except Exception:
                    pass

            for index, media_path in enumerate(self.files, start=1):
                self._queue_status(f"Transcribing {index}/{len(self.files)}", media_path.name)
                self._queue_log(f"Started {index}/{len(self.files)}: {media_path.name}", "info")
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
                output_dir = media_path.parent if self.output_mode_same_folder.get() else Path(self.output_dir.get()).resolve()
                output_dir.mkdir(parents=True, exist_ok=True)
                output_path = write_markdown_transcript(media_path, output_dir, model_name, device, result, include_timecodes)
                self._queue_log(f"Saved: {output_path}", "success")
                if index < len(self.files) and buffer_seconds > 0:
                    self._queue_log(f"Waiting {buffer_seconds} seconds before next file ...", "info")
                    time.sleep(buffer_seconds)
            self._queue_status("Completed", self.file_count_text())
            self._queue_log("Bulk transcription completed.", "success")
        except Exception as exc:
            self._queue_status("Failed", "See Activity for details")
            self._queue_log(f"Error: {exc}", "error")
            self._queue_log(traceback.format_exc(), "error")
        finally:
            self.log_queue.put(("done", None))

    def _queue_log(self, message: str, level: str) -> None:
        self.log_queue.put(("log", f"{level}|{message}"))

    def _queue_status(self, status: str, progress: str) -> None:
        self.log_queue.put(("status", f"{status}|{progress}"))

    def _poll_log_queue(self) -> None:
        while True:
            try:
                kind, message = self.log_queue.get_nowait()
            except queue.Empty:
                break
            if kind == "log" and message is not None:
                level, text = message.split("|", 1)
                self._log(text, level)
            elif kind == "status" and message is not None:
                status, progress = message.split("|", 1)
                self.status.set(status)
                self.progress.set(progress)
            elif kind == "done":
                self.start_button.configure(state="normal")
        self.root.after(150, self._poll_log_queue)

    def _log(self, message: str, level: str) -> None:
        timestamp = datetime.now().strftime("%H:%M:%S")
        prefix = {"info": "INFO", "success": "OK", "error": "ERROR"}.get(level, "INFO")
        self.log_text.configure(state="normal")
        self.log_text.insert(END, f"[{timestamp}] {prefix}: {message}\n")
        self.log_text.configure(state="disabled")
        self.log_text.see(END)

    def file_count_text(self) -> str:
        if not self.files:
            return "No files selected"
        if len(self.files) == 1:
            return "1 file selected"
        return f"{len(self.files)} files selected"


def main() -> None:
    root = Tk()
    TranscriptionApp(root)
    root.mainloop()


if __name__ == "__main__":
    main()
