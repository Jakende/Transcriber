from __future__ import annotations

import copy
import os
import queue
import subprocess
import sys
import threading
import traceback
from datetime import datetime
from pathlib import Path
from tkinter import (
    BOTH,
    END,
    LEFT,
    BooleanVar,
    DoubleVar,
    Listbox,
    StringVar,
    Text,
    Tk,
    Toplevel,
    filedialog,
    messagebox,
)
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

_APP_DIR = Path(__file__).resolve().parent
if str(_APP_DIR) not in sys.path:
    sys.path.insert(0, str(_APP_DIR))


from windows_services import (  # noqa: E402
    APP_TITLE,
    SUPPORTED_EXTENSIONS,
    TOOLS,
    PodcastFeed,
    QueueItem,
    ResultRecord,
    Transcriber,
    TranscriptionOptions,
    WindowsStore,
    annotate_segment_languages,
    delete_internal_record,
    diagnostics_report,
    download_podcast_episode,
    download_youtube,
    export_document,
    glossary_candidates,
    inspect_youtube,
    load_podcast_credentials,
    load_results,
    load_rss_feed,
    merge_segment_with_next,
    merge_speakers,
    parse_vtt,
    podcast_index_search,
    read_document,
    refresh_existing_outputs,
    replace_terms,
    result_from_document,
    save_document,
    save_podcast_credentials,
    speaker_labels,
    split_segment,
    write_document,
)


MEDIA_FILETYPES = [
    ("Audio- und Videodateien", " ".join(f"*{suffix}" for suffix in sorted(SUPPORTED_EXTENSIONS))),
    ("Alle Dateien", "*.*"),
]
OUTPUT_FORMATS = ("markdown", "vtt", "srt", "txt", "csv")
SPEAKER_RANGES = {
    "Automatisch": "auto",
    "Genau 2": "2-2",
    "2–4": "2-4",
    "4–6": "4-6",
    "5–8": "5-8",
    "6–10": "6-10",
}
SEPARATION_PRESETS = {
    "Locker – ähnliche Stimmen zusammen": 0.78,
    "Normal – ähnliche Stimmen eher zusammen": 0.60,
    "Streng – mehr Trennung": 0.45,
    "Sehr streng": 0.30,
}


def _supported_drop_paths(raw: str, root: Tk) -> list[Path]:
    try:
        values = root.tk.splitlist(raw)
    except Exception:
        values = [raw]
    return [Path(value) for value in values if Path(value).suffix.casefold() in SUPPORTED_EXTENSIONS]


class TranscriptionApp:
    def __init__(self, root: Tk) -> None:
        self.root = root
        self.root.title(APP_TITLE)
        self.root.geometry("1280x850")
        self.root.minsize(1040, 700)
        self.store = WindowsStore()
        self.files = self.store.load_queue()
        self.results = load_results()
        self.archived_ids = self.store.archived_ids()
        self.events: queue.Queue[tuple[str, object]] = queue.Queue()
        self.worker: threading.Thread | None = None
        self.cancel_event = threading.Event()

        preferences = self.store.load_preferences()
        self.language = StringVar(value=str(preferences.get("language", "de")))
        self.model_name = StringVar(value=str(preferences.get("model", "turbo")))
        self.diarization = BooleanVar(value=bool(preferences.get("diarization", True)))
        self.speaker_range_title = StringVar(value=str(preferences.get("speaker_range_title", "Automatisch")))
        self.separation_title = StringVar(value=str(preferences.get("separation_title", "Normal – ähnliche Stimmen eher zusammen")))
        self.include_timecodes = BooleanVar(value=bool(preferences.get("timecodes", True)))
        self.use_source_folder = BooleanVar(value=bool(preferences.get("use_source_folder", True)))
        self.output_dir = StringVar(value=str(preferences.get("output_dir", "")))
        self.formats = {
            name: BooleanVar(value=bool(preferences.get(f"format_{name}", name == "markdown")))
            for name in OUTPUT_FORMATS
        }
        self.status = StringVar(value="Bereit")
        self.progress_line = StringVar(value="Keine Verarbeitung aktiv")
        self.progress_value = DoubleVar(value=0)
        self.result_search = StringVar(value="")
        self.show_archived = BooleanVar(value=False)

        self._build_ui()
        self._refresh_queue()
        self._refresh_results()
        self._check_environment()
        self._poll_events()
        self.root.protocol("WM_DELETE_WINDOW", self._close)

    def _build_ui(self) -> None:
        style = ttk.Style()
        if "vista" in style.theme_names():
            style.theme_use("vista")
        style.configure("Title.TLabel", font=("Segoe UI", 22, "bold"))
        style.configure("Heading.TLabel", font=("Segoe UI", 11, "bold"))
        style.configure("Primary.TButton", font=("Segoe UI", 10, "bold"))

        outer = ttk.Frame(self.root, padding=18)
        outer.pack(fill=BOTH, expand=True)
        outer.columnconfigure(0, weight=1)
        outer.rowconfigure(1, weight=1)

        header = ttk.Frame(outer)
        header.grid(row=0, column=0, sticky="ew", pady=(0, 12))
        header.columnconfigure(0, weight=1)
        ttk.Label(header, text=APP_TITLE, style="Title.TLabel").grid(row=0, column=0, sticky="w")
        ttk.Label(header, text="Lokale Transkription mit Sprechererkennung und vollständiger Nachbearbeitung.").grid(row=1, column=0, sticky="w")
        actions = ttk.Frame(header)
        actions.grid(row=0, column=1, rowspan=2, sticky="e")
        for text, command in (
            ("Dateien hinzufügen", self.select_files),
            ("Medien laden", self.open_media_browser),
            ("VTT importieren", self.import_vtt),
            ("Wörterbuch", self.open_glossary_manager),
            ("System prüfen", self.open_diagnostics),
        ):
            ttk.Button(actions, text=text, command=command).pack(side=LEFT, padx=(6, 0))

        body = ttk.Panedwindow(outer, orient="horizontal")
        body.grid(row=1, column=0, sticky="nsew")
        queue_panel = ttk.Frame(body, padding=(0, 0, 10, 0))
        detail_panel = ttk.Frame(body, padding=(10, 0, 0, 0))
        body.add(queue_panel, weight=2)
        body.add(detail_panel, weight=5)

        queue_panel.columnconfigure(0, weight=1)
        queue_panel.rowconfigure(1, weight=1)
        ttk.Label(queue_panel, text="Warteschlange", style="Heading.TLabel").grid(row=0, column=0, sticky="w", pady=(0, 8))
        self.file_tree = ttk.Treeview(queue_panel, columns=("folder",), show="tree headings", selectmode="browse")
        self.file_tree.heading("#0", text="Datei")
        self.file_tree.heading("folder", text="Ordner")
        self.file_tree.column("#0", width=190, stretch=True)
        self.file_tree.column("folder", width=220, stretch=True)
        self.file_tree.grid(row=1, column=0, sticky="nsew")
        file_scroll = ttk.Scrollbar(queue_panel, orient="vertical", command=self.file_tree.yview)
        file_scroll.grid(row=1, column=1, sticky="ns")
        self.file_tree.configure(yscrollcommand=file_scroll.set)
        queue_actions = ttk.Frame(queue_panel)
        queue_actions.grid(row=2, column=0, columnspan=2, sticky="ew", pady=(8, 0))
        ttk.Button(queue_actions, text="↑", width=3, command=lambda: self.move_file(-1)).pack(side=LEFT)
        ttk.Button(queue_actions, text="↓", width=3, command=lambda: self.move_file(1)).pack(side=LEFT, padx=(4, 10))
        ttk.Button(queue_actions, text="Entfernen", command=self.remove_selected_file).pack(side=LEFT)
        ttk.Button(queue_actions, text="Leeren", command=self.clear_files).pack(side="right")
        self._enable_drop(self.file_tree)

        detail_panel.columnconfigure(0, weight=1)
        detail_panel.rowconfigure(2, weight=1)
        self._build_settings(detail_panel)
        self._build_run_bar(detail_panel)
        notebook = ttk.Notebook(detail_panel)
        notebook.grid(row=2, column=0, sticky="nsew")
        results_tab = ttk.Frame(notebook, padding=10)
        log_tab = ttk.Frame(notebook, padding=10)
        notebook.add(results_tab, text="Ergebnisse")
        notebook.add(log_tab, text="Aktivität")
        self._build_results(results_tab)
        self._build_log(log_tab)

    def _build_settings(self, parent: ttk.Frame) -> None:
        panel = ttk.LabelFrame(parent, text="Einstellungen", padding=12)
        panel.grid(row=0, column=0, sticky="ew", pady=(0, 12))
        panel.columnconfigure(7, weight=1)
        ttk.Label(panel, text="Sprache").grid(row=0, column=0, sticky="w")
        ttk.Combobox(panel, textvariable=self.language, values=("de", "en", "auto"), state="readonly", width=9).grid(row=1, column=0, sticky="w", padx=(0, 14))
        ttk.Label(panel, text="Whisper-Modell").grid(row=0, column=1, sticky="w")
        ttk.Combobox(panel, textvariable=self.model_name, values=("turbo", "medium", "small"), state="readonly", width=11).grid(row=1, column=1, sticky="w", padx=(0, 14))
        ttk.Checkbutton(panel, text="Sprecher:innen erkennen", variable=self.diarization, command=self._toggle_diarization).grid(row=1, column=2, sticky="w", padx=(0, 14))
        self.speaker_range_box = ttk.Combobox(panel, textvariable=self.speaker_range_title, values=tuple(SPEAKER_RANGES), state="readonly", width=14)
        self.speaker_range_box.grid(row=1, column=3, sticky="w", padx=(0, 14))
        self.separation_box = ttk.Combobox(panel, textvariable=self.separation_title, values=tuple(SEPARATION_PRESETS), state="readonly", width=37)
        self.separation_box.grid(row=1, column=4, columnspan=3, sticky="w")

        ttk.Label(panel, text="Ausgaben").grid(row=2, column=0, sticky="w", pady=(12, 0))
        format_bar = ttk.Frame(panel)
        format_bar.grid(row=3, column=0, columnspan=5, sticky="w")
        for name in OUTPUT_FORMATS:
            ttk.Checkbutton(format_bar, text="Markdown" if name == "markdown" else name.upper(), variable=self.formats[name]).pack(side=LEFT, padx=(0, 10))
        ttk.Checkbutton(format_bar, text="Zeitcodes", variable=self.include_timecodes).pack(side=LEFT, padx=(8, 0))

        ttk.Label(panel, text="Speicherort").grid(row=4, column=0, sticky="w", pady=(12, 0))
        ttk.Checkbutton(panel, text="Neben Quelldatei", variable=self.use_source_folder, command=self._toggle_output_picker).grid(row=5, column=0, columnspan=2, sticky="w")
        self.output_entry = ttk.Entry(panel, textvariable=self.output_dir)
        self.output_entry.grid(row=5, column=2, columnspan=4, sticky="ew", padx=(8, 8))
        self.output_button = ttk.Button(panel, text="Ordner wählen", command=self.select_output_dir)
        self.output_button.grid(row=5, column=6, sticky="e")
        self._toggle_output_picker()
        self._toggle_diarization()

    def _build_run_bar(self, parent: ttk.Frame) -> None:
        bar = ttk.Frame(parent)
        bar.grid(row=1, column=0, sticky="ew", pady=(0, 12))
        bar.columnconfigure(0, weight=1)
        ttk.Label(bar, textvariable=self.status, style="Heading.TLabel").grid(row=0, column=0, sticky="w")
        ttk.Label(bar, textvariable=self.progress_line).grid(row=1, column=0, sticky="w", pady=(2, 4))
        ttk.Progressbar(bar, variable=self.progress_value, maximum=1).grid(row=2, column=0, sticky="ew", padx=(0, 12))
        self.cancel_button = ttk.Button(bar, text="Abbrechen", command=self.cancel, state="disabled")
        self.cancel_button.grid(row=0, column=1, rowspan=3, padx=(0, 8))
        self.start_button = ttk.Button(bar, text="Transkription starten", command=self.start_transcription, style="Primary.TButton")
        self.start_button.grid(row=0, column=2, rowspan=3)

    def _build_results(self, parent: ttk.Frame) -> None:
        parent.columnconfigure(0, weight=1)
        parent.rowconfigure(1, weight=1)
        filters = ttk.Frame(parent)
        filters.grid(row=0, column=0, sticky="ew", pady=(0, 8))
        filters.columnconfigure(0, weight=1)
        search = ttk.Entry(filters, textvariable=self.result_search)
        search.grid(row=0, column=0, sticky="ew", padx=(0, 10))
        search.bind("<KeyRelease>", lambda _event: self._refresh_results())
        ttk.Checkbutton(filters, text="Archiv anzeigen", variable=self.show_archived, command=self._refresh_results).grid(row=0, column=1)
        self.result_tree = ttk.Treeview(parent, columns=("segments", "speakers", "formats"), show="tree headings", selectmode="browse")
        self.result_tree.heading("#0", text="Quelldatei")
        self.result_tree.heading("segments", text="Segmente")
        self.result_tree.heading("speakers", text="Sprecher:innen")
        self.result_tree.heading("formats", text="Ausgaben")
        self.result_tree.column("#0", width=330)
        self.result_tree.column("segments", width=80, anchor="center")
        self.result_tree.column("speakers", width=100, anchor="center")
        self.result_tree.column("formats", width=160)
        self.result_tree.grid(row=1, column=0, sticky="nsew")
        self.result_tree.bind("<Double-1>", lambda _event: self.open_editor())
        actions = ttk.Frame(parent)
        actions.grid(row=2, column=0, sticky="ew", pady=(8, 0))
        for text, command in (
            ("Bearbeiten", self.open_editor),
            ("Im Explorer", self.reveal_result),
            ("Archivieren / zurückholen", self.toggle_archive),
            ("Bearbeitungsstand löschen …", self.delete_result),
        ):
            ttk.Button(actions, text=text, command=command).pack(side=LEFT, padx=(0, 8))

    def _build_log(self, parent: ttk.Frame) -> None:
        parent.columnconfigure(0, weight=1)
        parent.rowconfigure(0, weight=1)
        self.log_text = Text(parent, wrap="word", font=("Consolas", 9), state="disabled", padx=10, pady=8)
        self.log_text.grid(row=0, column=0, sticky="nsew")
        scroll = ttk.Scrollbar(parent, orient="vertical", command=self.log_text.yview)
        scroll.grid(row=0, column=1, sticky="ns")
        self.log_text.configure(yscrollcommand=scroll.set)
        ttk.Button(parent, text="Protokoll leeren", command=self.clear_log).grid(row=1, column=0, sticky="e", pady=(8, 0))

    def _enable_drop(self, widget: ttk.Treeview) -> None:
        try:
            from tkinterdnd2 import DND_FILES
            widget.drop_target_register(DND_FILES)
            widget.dnd_bind("<<Drop>>", lambda event: self.add_paths(_supported_drop_paths(event.data, self.root)))
        except (ImportError, AttributeError):
            pass

    def _check_environment(self) -> None:
        for name in ("ffmpeg", "ffprobe", "ffplay"):
            if TOOLS.get(name):
                self._log(f"{name} aktiv: {TOOLS[name]}", "success")
            else:
                self._log(f"{name} wurde nicht gefunden; zugehörige Funktionen sind nicht verfügbar.", "error")
        self._log(f"{len(self.results)} gespeicherte Ergebnisse wiederhergestellt.", "info")

    def _toggle_diarization(self) -> None:
        state = "readonly" if self.diarization.get() else "disabled"
        self.speaker_range_box.configure(state=state)
        self.separation_box.configure(state=state)

    def _toggle_output_picker(self) -> None:
        state = "disabled" if self.use_source_folder.get() else "normal"
        self.output_entry.configure(state=state)
        self.output_button.configure(state=state)

    def select_files(self) -> None:
        values = filedialog.askopenfilenames(title="Audio- und Videodateien auswählen", filetypes=MEDIA_FILETYPES)
        self.add_paths(Path(value) for value in values)

    def add_paths(self, paths) -> None:
        known = {item.source.resolve() for item in self.files}
        additions = 0
        rejected = 0
        for raw in paths:
            path = Path(raw).resolve()
            if not path.is_file() or path.suffix.casefold() not in SUPPORTED_EXTENSIONS:
                rejected += 1
                continue
            if path in known:
                continue
            self.files.append(QueueItem(str(path), self.store.podcast_metadata(path)))
            known.add(path)
            additions += 1
        self.store.save_queue(self.files)
        self._refresh_queue()
        if additions:
            self._log(f"{additions} Datei(en) hinzugefügt.", "info")
        if rejected:
            self._log(f"{rejected} nicht unterstützte Datei(en) übersprungen.", "error")

    def add_downloaded_items(self, items: list[QueueItem], autostart: bool = False) -> None:
        for item in items:
            if item.podcast:
                self.store.register_podcast(item.source, item.podcast)
        self.add_paths(item.source for item in items)
        if autostart and items:
            self.root.after(100, lambda: self._start_files(items))

    def remove_selected_file(self) -> None:
        selected = self.file_tree.selection()
        if not selected:
            return
        index = int(selected[0])
        if 0 <= index < len(self.files):
            self.files.pop(index)
            self.store.save_queue(self.files)
            self._refresh_queue()

    def clear_files(self) -> None:
        if self.worker and self.worker.is_alive():
            return
        self.files.clear()
        self.store.save_queue(self.files)
        self._refresh_queue()

    def move_file(self, offset: int) -> None:
        selected = self.file_tree.selection()
        if not selected:
            return
        index = int(selected[0])
        target = index + offset
        if 0 <= target < len(self.files):
            self.files[index], self.files[target] = self.files[target], self.files[index]
            self.store.save_queue(self.files)
            self._refresh_queue(select=target)

    def _refresh_queue(self, select: int | None = None) -> None:
        self.file_tree.delete(*self.file_tree.get_children())
        for index, item in enumerate(self.files):
            label = item.source.name + ("  🎙" if item.podcast else "")
            self.file_tree.insert("", END, iid=str(index), text=label, values=(str(item.source.parent),))
        if select is not None and str(select) in self.file_tree.get_children():
            self.file_tree.selection_set(str(select))
        self.status.set(f"{len(self.files)} Datei(en) ausgewählt" if self.files else "Keine Dateien ausgewählt")

    def select_output_dir(self) -> None:
        value = filedialog.askdirectory(title="Zielordner wählen", initialdir=self.output_dir.get() or None)
        if value:
            self.output_dir.set(value)

    def _save_preferences(self) -> None:
        values = {
            "language": self.language.get(), "model": self.model_name.get(), "diarization": self.diarization.get(),
            "speaker_range_title": self.speaker_range_title.get(), "separation_title": self.separation_title.get(),
            "timecodes": self.include_timecodes.get(), "use_source_folder": self.use_source_folder.get(), "output_dir": self.output_dir.get(),
        }
        values.update({f"format_{name}": variable.get() for name, variable in self.formats.items()})
        self.store.save_preferences(values)

    def _options(self) -> TranscriptionOptions:
        formats = {name for name, variable in self.formats.items() if variable.get()}
        if not formats:
            raise ValueError("Mindestens ein Ausgabeformat muss aktiviert sein.")
        if not self.use_source_folder.get() and not self.output_dir.get().strip():
            raise ValueError("Bitte einen Zielordner wählen.")
        return TranscriptionOptions(
            language=self.language.get(), model=self.model_name.get(), include_timecodes=self.include_timecodes.get(),
            diarize=self.diarization.get(), speaker_range=SPEAKER_RANGES[self.speaker_range_title.get()],
            cluster_threshold=SEPARATION_PRESETS[self.separation_title.get()], formats=formats,
            output_dir=None if self.use_source_folder.get() else Path(self.output_dir.get()).resolve(),
        )

    def start_transcription(self) -> None:
        self._start_files(list(self.files))

    def _start_files(self, files: list[QueueItem]) -> None:
        if self.worker and self.worker.is_alive():
            return
        if not files:
            messagebox.showinfo(APP_TITLE, "Bitte zuerst mindestens eine Datei hinzufügen.", parent=self.root)
            return
        try:
            options = self._options()
        except ValueError as error:
            messagebox.showerror(APP_TITLE, str(error), parent=self.root)
            return
        self._save_preferences()
        self.cancel_event.clear()
        self.start_button.configure(state="disabled")
        self.cancel_button.configure(state="normal")
        self.progress_value.set(0)
        self.status.set("Läuft …")
        self.worker = threading.Thread(target=self._run_transcription, args=(files, options), daemon=True)
        self.worker.start()

    def _run_transcription(self, files: list[QueueItem], options: TranscriptionOptions) -> None:
        current_index = 0
        def log(message: str, level: str) -> None:
            self.events.put(("log", (message, level)))
        def progress(percent: int, message: str) -> None:
            self.events.put(("progress", ((current_index + percent / 100) / max(1, len(files)), f"Datei {current_index + 1} von {len(files)}: {message}")))
        try:
            transcriber = Transcriber(log, progress)
            transcriber.load_model(options.model)
            succeeded = failed = 0
            for index, item in enumerate(files):
                current_index = index
                if self.cancel_event.is_set():
                    break
                log(f"Starte {index + 1}/{len(files)}: {item.source.name}", "info")
                try:
                    result = transcriber.transcribe_one(item, options, self.store, self.cancel_event)
                    succeeded += 1
                    self.events.put(("result", result))
                    log(f"Gespeichert: {item.source.name}", "success")
                except InterruptedError:
                    break
                except Exception as error:
                    failed += 1
                    log(f"{item.source.name}: {error}", "error")
                    log(traceback.format_exc(), "error")
            if self.cancel_event.is_set():
                self.events.put(("status", ("Abgebrochen", "Fertige Ergebnisse bleiben erhalten.")))
            else:
                self.events.put(("status", ("Abgeschlossen", f"{succeeded} erfolgreich, {failed} fehlgeschlagen")))
        except Exception as error:
            log(str(error), "error")
            log(traceback.format_exc(), "error")
            self.events.put(("status", ("Fehlgeschlagen", "Details stehen im Aktivitätsprotokoll.")))
        finally:
            self.events.put(("done", None))

    def cancel(self) -> None:
        self.cancel_event.set()
        self.progress_line.set("Abbruch angefordert; die aktuelle Modelloperation wird noch beendet …")
        self.cancel_button.configure(state="disabled")

    def _poll_events(self) -> None:
        while True:
            try:
                kind, payload = self.events.get_nowait()
            except queue.Empty:
                break
            if kind == "log":
                message, level = payload
                self._log(message, level)
            elif kind == "progress":
                value, message = payload
                self.progress_value.set(value)
                self.progress_line.set(message)
            elif kind == "result":
                self.results.insert(0, payload)
                self._refresh_results()
            elif kind == "status":
                status, line = payload
                self.status.set(status)
                self.progress_line.set(line)
                if status == "Abgeschlossen":
                    self.progress_value.set(1)
            elif kind == "done":
                self.start_button.configure(state="normal")
                self.cancel_button.configure(state="disabled")
        self.root.after(120, self._poll_events)

    def _refresh_results(self) -> None:
        if not hasattr(self, "result_tree"):
            return
        self.result_tree.delete(*self.result_tree.get_children())
        query = self.result_search.get().casefold().strip()
        for result in self.results:
            archived = result.document_id in self.archived_ids
            if archived and not self.show_archived.get():
                continue
            if query and query not in result.source_path.name.casefold() and query not in str(result.source_path).casefold():
                continue
            label = ("[Archiv] " if archived else "") + result.source_path.name
            self.result_tree.insert("", END, iid=result.document_id, text=label, values=(result.segment_count, result.speaker_count, ", ".join(sorted(result.outputs))))

    def selected_result(self) -> ResultRecord | None:
        selected = self.result_tree.selection()
        if not selected:
            return None
        return next((item for item in self.results if item.document_id == selected[0]), None)

    def open_editor(self) -> None:
        result = self.selected_result()
        if result:
            TranscriptEditor(self, result)

    def result_updated(self, document_path: Path) -> None:
        updated = result_from_document(document_path)
        self.results = [updated if item.document_id == updated.document_id else item for item in self.results]
        self._refresh_results()

    def reveal_result(self) -> None:
        result = self.selected_result()
        if not result:
            return
        target = Path(next(iter(result.outputs.values()), result.source_path))
        try:
            os.startfile(str(target.parent if target.is_file() else target))  # type: ignore[attr-defined]
        except (AttributeError, OSError):
            subprocess.Popen(["explorer", "/select,", str(target)])

    def toggle_archive(self) -> None:
        result = self.selected_result()
        if not result:
            return
        if result.document_id in self.archived_ids:
            self.archived_ids.remove(result.document_id)
        else:
            self.archived_ids.add(result.document_id)
        self.store.save_archived_ids(self.archived_ids)
        self._refresh_results()

    def delete_result(self) -> None:
        result = self.selected_result()
        if not result:
            return
        if not messagebox.askyesno(APP_TITLE, "Den internen Bearbeitungsstand in den Papierkorb verschieben? Quelldatei und Exporte bleiben erhalten.", parent=self.root):
            return
        try:
            delete_internal_record(result.document_path)
            self.results.remove(result)
            self.archived_ids.discard(result.document_id)
            self.store.save_archived_ids(self.archived_ids)
            self._refresh_results()
        except Exception as error:
            messagebox.showerror(APP_TITLE, str(error), parent=self.root)

    def import_vtt(self) -> None:
        selected = filedialog.askopenfilename(title="VTT-Datei importieren", filetypes=[("WebVTT", "*.vtt")])
        if not selected:
            return
        audio = None
        if messagebox.askyesno(APP_TITLE, "Soll eine zugehörige Audio- oder Videodatei ausgewählt werden?", parent=self.root):
            value = filedialog.askopenfilename(title="Zugehörige Mediendatei", filetypes=MEDIA_FILETYPES)
            audio = Path(value) if value else None
        try:
            document = parse_vtt(Path(selected), audio)
            path = write_document(document, Path(self.store.root) / "Jobs")
            self.results.insert(0, result_from_document(path))
            self._refresh_results()
            self._log(f"VTT importiert: {Path(selected).name}", "success")
        except Exception as error:
            messagebox.showerror(APP_TITLE, str(error), parent=self.root)

    def open_media_browser(self) -> None:
        MediaBrowser(self)

    def open_glossary_manager(self) -> None:
        GlossaryManager(self)

    def open_diagnostics(self) -> None:
        window = Toplevel(self.root)
        window.title("Systemprüfung")
        window.geometry("820x560")
        text = Text(window, wrap="none", font=("Consolas", 9), padx=12, pady=12)
        text.pack(fill=BOTH, expand=True)
        text.insert("1.0", diagnostics_report())
        text.configure(state="disabled")
        ttk.Button(window, text="Bericht kopieren", command=lambda: self._copy_to_clipboard(text.get("1.0", END))).pack(pady=8)

    def _copy_to_clipboard(self, value: str) -> None:
        self.root.clipboard_clear()
        self.root.clipboard_append(value)

    def clear_log(self) -> None:
        self.log_text.configure(state="normal")
        self.log_text.delete("1.0", END)
        self.log_text.configure(state="disabled")

    def _log(self, message: str, level: str) -> None:
        stamp = datetime.now().strftime("%H:%M:%S")
        prefix = {"success": "OK", "error": "FEHLER"}.get(level, "INFO")
        self.log_text.configure(state="normal")
        self.log_text.insert(END, f"[{stamp}] {prefix}: {message.rstrip()}\n")
        self.log_text.configure(state="disabled")
        self.log_text.see(END)

    def _close(self) -> None:
        self._save_preferences()
        self.root.destroy()


class GlossaryManager(Toplevel):
    def __init__(self, app: TranscriptionApp) -> None:
        super().__init__(app.root)
        self.app = app
        self.title("Wörterbuch")
        self.geometry("620x520")
        self.transient(app.root)
        self.columnconfigure(0, weight=1)
        self.rowconfigure(1, weight=1)
        entry_bar = ttk.Frame(self, padding=12)
        entry_bar.grid(row=0, column=0, sticky="ew")
        entry_bar.columnconfigure(0, weight=1)
        self.term = StringVar()
        ttk.Entry(entry_bar, textvariable=self.term).grid(row=0, column=0, sticky="ew", padx=(0, 8))
        ttk.Button(entry_bar, text="Hinzufügen", command=self.add).grid(row=0, column=1)
        self.listbox = Listbox(self, selectmode="extended")
        self.listbox.grid(row=1, column=0, sticky="nsew", padx=12)
        actions = ttk.Frame(self, padding=12)
        actions.grid(row=2, column=0, sticky="ew")
        ttk.Button(actions, text="Entfernen", command=self.remove).pack(side=LEFT)
        ttk.Button(actions, text="Importieren …", command=self.import_terms).pack(side="right", padx=(8, 0))
        ttk.Button(actions, text="Exportieren …", command=self.export_terms).pack(side="right")
        self.refresh()

    def refresh(self) -> None:
        self.listbox.delete(0, END)
        for term in self.app.store.glossary_terms():
            self.listbox.insert(END, term)

    def add(self) -> None:
        terms = self.app.store.glossary_terms()
        if self.term.get().strip():
            terms.append(self.term.get().strip())
            self.app.store.save_glossary_terms(terms)
            self.term.set("")
            self.refresh()

    def remove(self) -> None:
        remove = {self.listbox.get(index) for index in self.listbox.curselection()}
        self.app.store.save_glossary_terms(term for term in self.app.store.glossary_terms() if term not in remove)
        self.refresh()

    def import_terms(self) -> None:
        path = filedialog.askopenfilename(parent=self, filetypes=[("Text", "*.txt"), ("Alle Dateien", "*.*")])
        if path:
            self.app.store.save_glossary_terms(self.app.store.glossary_terms() + Path(path).read_text(encoding="utf-8-sig").splitlines())
            self.refresh()

    def export_terms(self) -> None:
        path = filedialog.asksaveasfilename(parent=self, defaultextension=".txt", filetypes=[("Text", "*.txt")])
        if path:
            Path(path).write_text("\n".join(self.app.store.glossary_terms()) + "\n", encoding="utf-8")


class MediaBrowser(Toplevel):
    def __init__(self, app: TranscriptionApp) -> None:
        super().__init__(app.root)
        self.app = app
        self.title("Medien laden")
        self.geometry("980x720")
        self.minsize(820, 620)
        self.transient(app.root)
        self.feed: PodcastFeed | None = None
        self.youtube_info: dict | None = None
        preferences = app.store.load_preferences()
        self.folder = StringVar(value=str(preferences.get("download_folder", Path.home() / "Downloads")))
        self.autostart = BooleanVar(value=bool(preferences.get("download_autostart", True)))
        self.status = StringVar(value="Bereit")
        self.progress = DoubleVar(value=0)
        self.columnconfigure(0, weight=1)
        self.rowconfigure(0, weight=1)
        notebook = ttk.Notebook(self)
        notebook.grid(row=0, column=0, sticky="nsew", padx=12, pady=12)
        rss, index, youtube = ttk.Frame(notebook, padding=12), ttk.Frame(notebook, padding=12), ttk.Frame(notebook, padding=12)
        notebook.add(rss, text="RSS-Feed")
        notebook.add(index, text="PodcastIndex")
        notebook.add(youtube, text="YouTube")
        self._build_rss(rss)
        self._build_index(index)
        self._build_youtube(youtube)
        footer = ttk.Frame(self, padding=(12, 0, 12, 12))
        footer.grid(row=1, column=0, sticky="ew")
        footer.columnconfigure(1, weight=1)
        ttk.Button(footer, text="Downloadordner …", command=self.choose_folder).grid(row=0, column=0)
        ttk.Label(footer, textvariable=self.folder).grid(row=0, column=1, sticky="w", padx=8)
        ttk.Checkbutton(footer, text="Danach transkribieren", variable=self.autostart).grid(row=0, column=2)
        ttk.Progressbar(footer, variable=self.progress, maximum=1).grid(row=1, column=0, columnspan=3, sticky="ew", pady=(8, 3))
        ttk.Label(footer, textvariable=self.status).grid(row=2, column=0, columnspan=3, sticky="w")

    def _build_rss(self, parent: ttk.Frame) -> None:
        parent.columnconfigure(0, weight=1)
        parent.rowconfigure(1, weight=1)
        bar = ttk.Frame(parent)
        bar.grid(row=0, column=0, sticky="ew", pady=(0, 8))
        bar.columnconfigure(0, weight=1)
        self.rss_url = StringVar()
        ttk.Entry(bar, textvariable=self.rss_url).grid(row=0, column=0, sticky="ew", padx=(0, 8))
        ttk.Button(bar, text="Feed laden", command=self.load_rss).grid(row=0, column=1)
        self.episode_tree = ttk.Treeview(parent, columns=("date", "duration"), show="tree headings", selectmode="extended")
        for column, title in (("#0", "Folge"), ("date", "Veröffentlicht"), ("duration", "Dauer")):
            self.episode_tree.heading(column, text=title)
        self.episode_tree.column("#0", width=560)
        self.episode_tree.column("date", width=160)
        self.episode_tree.column("duration", width=90)
        self.episode_tree.grid(row=1, column=0, sticky="nsew")
        ttk.Button(parent, text="Ausgewählte Folgen laden", command=self.download_episodes).grid(row=2, column=0, sticky="e", pady=(8, 0))

    def _build_index(self, parent: ttk.Frame) -> None:
        parent.columnconfigure(0, weight=1)
        parent.rowconfigure(2, weight=1)
        key, secret = load_podcast_credentials()
        credentials = ttk.LabelFrame(parent, text="PodcastIndex-Zugang", padding=8)
        credentials.grid(row=0, column=0, sticky="ew", pady=(0, 10))
        credentials.columnconfigure(1, weight=1)
        self.api_key, self.api_secret = StringVar(value=key), StringVar(value=secret)
        ttk.Label(credentials, text="Key").grid(row=0, column=0, sticky="w")
        ttk.Entry(credentials, textvariable=self.api_key).grid(row=0, column=1, sticky="ew", padx=8)
        ttk.Label(credentials, text="Secret").grid(row=1, column=0, sticky="w")
        ttk.Entry(credentials, textvariable=self.api_secret, show="•").grid(row=1, column=1, sticky="ew", padx=8)
        ttk.Button(credentials, text="Sicher speichern", command=self.save_credentials).grid(row=0, column=2, rowspan=2)
        search = ttk.Frame(parent)
        search.grid(row=1, column=0, sticky="ew", pady=(0, 8))
        search.columnconfigure(0, weight=1)
        self.search_term = StringVar()
        ttk.Entry(search, textvariable=self.search_term).grid(row=0, column=0, sticky="ew", padx=(0, 8))
        ttk.Button(search, text="Suchen", command=self.search_podcasts).grid(row=0, column=1)
        self.search_tree = ttk.Treeview(parent, columns=("author", "url"), show="tree headings", selectmode="browse")
        for column, title in (("#0", "Podcast"), ("author", "Autor:in"), ("url", "Feed")):
            self.search_tree.heading(column, text=title)
        self.search_tree.column("#0", width=300)
        self.search_tree.column("author", width=180)
        self.search_tree.column("url", width=360)
        self.search_tree.grid(row=2, column=0, sticky="nsew")
        ttk.Button(parent, text="Ausgewählten Feed im RSS-Reiter öffnen", command=self.open_search_result).grid(row=3, column=0, sticky="e", pady=(8, 0))

    def _build_youtube(self, parent: ttk.Frame) -> None:
        parent.columnconfigure(0, weight=1)
        self.youtube_url = StringVar()
        self.youtube_details = StringVar(value="Noch kein Video geprüft.")
        self.keep_video = BooleanVar(value=False)
        ttk.Label(parent, text="Link zu einem einzelnen YouTube-Video").grid(row=0, column=0, sticky="w")
        ttk.Entry(parent, textvariable=self.youtube_url).grid(row=1, column=0, sticky="ew", pady=(4, 8))
        ttk.Button(parent, text="Video prüfen", command=self.inspect_video).grid(row=2, column=0, sticky="w")
        ttk.Label(parent, textvariable=self.youtube_details, wraplength=760, justify="left").grid(row=3, column=0, sticky="w", pady=18)
        ttk.Checkbutton(parent, text="Videodatei zusätzlich speichern", variable=self.keep_video).grid(row=4, column=0, sticky="w")
        ttk.Button(parent, text="YouTube-Audio laden", command=self.download_video).grid(row=5, column=0, sticky="w", pady=(12, 0))

    def choose_folder(self) -> None:
        value = filedialog.askdirectory(parent=self, initialdir=self.folder.get() or None)
        if value:
            self.folder.set(value)
            preferences = self.app.store.load_preferences()
            preferences["download_folder"] = value
            preferences["download_autostart"] = self.autostart.get()
            self.app.store.save_preferences(preferences)

    def _background(self, action, success=None) -> None:
        def run() -> None:
            try:
                value = action()
                if success:
                    self.after(0, lambda: success(value))
            except Exception as error:
                self.after(0, lambda value=str(error): self.status.set(f"Fehler: {value}"))
        threading.Thread(target=run, daemon=True).start()

    def load_rss(self) -> None:
        self.status.set("RSS-Feed wird geladen …")
        self._background(lambda: load_rss_feed(self.rss_url.get()), self._show_feed)

    def _show_feed(self, feed: PodcastFeed) -> None:
        self.feed = feed
        self.episode_tree.delete(*self.episode_tree.get_children())
        for index, episode in enumerate(feed.episodes):
            duration = f"{episode.duration_seconds // 60}:{episode.duration_seconds % 60:02d}" if episode.duration_seconds else ""
            self.episode_tree.insert("", END, iid=str(index), text=episode.title, values=(episode.published_at or "", duration))
        self.status.set(f"{feed.title}: {len(feed.episodes)} Folgen")

    def download_episodes(self) -> None:
        if not self.feed or not self.episode_tree.selection():
            messagebox.showinfo(APP_TITLE, "Bitte mindestens eine Folge auswählen.", parent=self)
            return
        selected = [self.feed.episodes[int(index)] for index in self.episode_tree.selection()]
        folder = Path(self.folder.get())
        self.status.set("Folgen werden geladen …")
        def action() -> list[QueueItem]:
            items = []
            for index, episode in enumerate(selected):
                items.append(download_podcast_episode(self.feed, episode, folder, lambda value: self.after(0, self.progress.set, (index + value) / len(selected))))
            return items
        self._background(action, self._downloads_complete)

    def _downloads_complete(self, items: list[QueueItem]) -> None:
        self.progress.set(1)
        self.status.set(f"{len(items)} Download(s) abgeschlossen")
        self.app.add_downloaded_items(items, self.autostart.get())

    def save_credentials(self) -> None:
        try:
            save_podcast_credentials(self.api_key.get(), self.api_secret.get())
            self.status.set("PodcastIndex-Zugang sicher gespeichert.")
        except Exception as error:
            self.status.set(f"Fehler: {error}")

    def search_podcasts(self) -> None:
        self.status.set("PodcastIndex wird durchsucht …")
        self._background(lambda: podcast_index_search(self.search_term.get(), self.api_key.get(), self.api_secret.get()), self._show_search_results)

    def _show_search_results(self, results: list[dict]) -> None:
        self.search_tree.delete(*self.search_tree.get_children())
        for index, item in enumerate(results):
            self.search_tree.insert("", END, iid=str(index), text=item.get("title") or "Podcast", values=(item.get("author") or "", item.get("url") or ""))
        self.search_results = results
        self.status.set(f"{len(results)} Treffer")

    def open_search_result(self) -> None:
        selected = self.search_tree.selection()
        results = getattr(self, "search_results", [])
        if not selected or int(selected[0]) >= len(results):
            return
        item = results[int(selected[0])]
        self.rss_url.set(str(item.get("url") or ""))
        self.status.set("Feed wird geladen …")
        self._background(lambda: load_rss_feed(self.rss_url.get(), int(item.get("id")) if item.get("id") else None), self._show_feed)

    def inspect_video(self) -> None:
        self.status.set("Videoinformationen werden geladen …")
        self._background(lambda: inspect_youtube(self.youtube_url.get()), self._show_youtube)

    def _show_youtube(self, info: dict) -> None:
        self.youtube_info = info
        duration = f"{int(info['duration']) // 60}:{int(info['duration']) % 60:02d}" if info.get("duration") else "unbekannt"
        self.youtube_details.set(f"{info.get('title')}\nKanal: {info.get('channel') or 'unbekannt'} · Dauer: {duration}")
        self.status.set("Bereit zum Download")

    def download_video(self) -> None:
        self.status.set("YouTube-Medium wird geladen …")
        self._background(
            lambda: download_youtube(self.youtube_url.get(), Path(self.folder.get()), self.keep_video.get(), lambda value: self.after(0, self.progress.set, value)),
            lambda value: self._downloads_complete([value[0]]),
        )


class AudioPlayer:
    RATES = (0.75, 1.0, 1.25, 1.5)
    def __init__(self, owner: Toplevel, source: Path) -> None:
        self.owner, self.source = owner, source
        self.process: subprocess.Popen | None = None
        self.start = self.end = 0.0
        self.rate = 1.0
        self.loop = False
    def play(self, start: float, end: float) -> None:
        self.stop()
        ffplay = TOOLS.get("ffplay")
        if not ffplay or not self.source.exists():
            return
        self.start, self.end = max(0, start), max(start, end)
        duration = max(0.1, self.end - self.start)
        self.process = subprocess.Popen([str(ffplay), "-nodisp", "-autoexit", "-loglevel", "error", "-ss", str(self.start), "-t", str(duration), "-af", f"atempo={self.rate}", str(self.source)], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        threading.Thread(target=self._monitor, args=(self.process,), daemon=True).start()
    def _monitor(self, process: subprocess.Popen) -> None:
        process.wait()
        if self.loop and process is self.process:
            self.owner.after(0, lambda: self.play(self.start, self.end))
    def stop(self) -> None:
        if self.process and self.process.poll() is None:
            self.process.terminate()
        self.process = None
    def seek(self, seconds: float) -> None:
        duration = max(0.1, self.end - self.start)
        self.play(max(0, self.start + seconds), max(0, self.start + seconds) + duration)
    def cycle_rate(self) -> float:
        index = self.RATES.index(self.rate) if self.rate in self.RATES else 1
        self.rate = self.RATES[(index + 1) % len(self.RATES)]
        return self.rate


class TranscriptEditor(Toplevel):
    def __init__(self, app: TranscriptionApp, result: ResultRecord) -> None:
        super().__init__(app.root)
        self.app, self.result = app, result
        self.document = read_document(result.document_path)
        annotate_segment_languages(self.document)
        self.undo_stack, self.redo_stack = [], []
        self.player = AudioPlayer(self, Path(self.document.source_path))
        self.title(f"Transkript bearbeiten – {result.source_path.name}")
        self.geometry("1220x800")
        self.minsize(960, 650)
        self.protocol("WM_DELETE_WINDOW", self.close)
        self.search, self.bulk_speaker = StringVar(), StringVar()
        self.segment_speaker, self.segment_language = StringVar(), StringVar()
        self.segment_start, self.segment_end = DoubleVar(), DoubleVar()
        self.status, self.rate_label, self.loop = StringVar(value="Bereit"), StringVar(value="1×"), BooleanVar(value=False)
        self._build()
        self.refresh_all()

    def _build(self) -> None:
        self.columnconfigure(0, weight=1)
        self.rowconfigure(1, weight=1)
        toolbar = ttk.Frame(self, padding=10)
        toolbar.grid(row=0, column=0, sticky="ew")
        for text, command in (("Rückgängig", self.undo), ("Wiederholen", self.redo), ("« 5 s", lambda: self.player.seek(-5)), ("Stopp", self.player.stop), ("5 s »", lambda: self.player.seek(5))):
            ttk.Button(toolbar, text=text, command=command).pack(side=LEFT, padx=(0, 4))
        ttk.Checkbutton(toolbar, text="Schleife", variable=self.loop, command=lambda: setattr(self.player, "loop", self.loop.get())).pack(side=LEFT, padx=(8, 4))
        ttk.Button(toolbar, textvariable=self.rate_label, command=self.cycle_rate).pack(side=LEFT)
        ttk.Button(toolbar, text="Exportieren …", command=self.export).pack(side="right")
        ttk.Button(toolbar, text="Speichern", command=lambda: self.persist(True)).pack(side="right", padx=8)
        ttk.Label(toolbar, textvariable=self.status).pack(side="right", padx=12)
        notebook = ttk.Notebook(self)
        notebook.grid(row=1, column=0, sticky="nsew", padx=10, pady=(0, 10))
        transcript, speakers, glossary = ttk.Frame(notebook, padding=8), ttk.Frame(notebook, padding=8), ttk.Frame(notebook, padding=8)
        notebook.add(transcript, text="Transkript")
        notebook.add(speakers, text="Sprecher:innen")
        notebook.add(glossary, text="Begriffe")
        self._build_transcript(transcript)
        self._build_speakers(speakers)
        self._build_glossary(glossary)

    def _build_transcript(self, parent: ttk.Frame) -> None:
        parent.columnconfigure(0, weight=3)
        parent.columnconfigure(1, weight=2)
        parent.rowconfigure(1, weight=1)
        filters = ttk.Frame(parent)
        filters.grid(row=0, column=0, columnspan=2, sticky="ew", pady=(0, 8))
        filters.columnconfigure(0, weight=1)
        entry = ttk.Entry(filters, textvariable=self.search)
        entry.grid(row=0, column=0, sticky="ew", padx=(0, 8))
        entry.bind("<KeyRelease>", lambda _event: self.refresh_segments())
        self.bulk_box = ttk.Combobox(filters, textvariable=self.bulk_speaker, state="readonly", width=22)
        self.bulk_box.grid(row=0, column=1, padx=(0, 4))
        ttk.Button(filters, text="Allen zuweisen", command=lambda: self.bulk_assign(False)).grid(row=0, column=2, padx=4)
        ttk.Button(filters, text="Nur leeren zuweisen", command=lambda: self.bulk_assign(True)).grid(row=0, column=3)
        self.segment_tree = ttk.Treeview(parent, columns=("time", "speaker", "language", "text"), show="headings", selectmode="browse")
        for name, title, width in (("time", "Zeit", 110), ("speaker", "Sprecher:in", 130), ("language", "Sprache", 65), ("text", "Text", 520)):
            self.segment_tree.heading(name, text=title)
            self.segment_tree.column(name, width=width, stretch=name == "text")
        self.segment_tree.grid(row=1, column=0, sticky="nsew", padx=(0, 8))
        self.segment_tree.bind("<<TreeviewSelect>>", lambda _event: self.load_segment())
        editor = ttk.LabelFrame(parent, text="Ausgewähltes Segment", padding=10)
        editor.grid(row=1, column=1, sticky="nsew")
        editor.columnconfigure(1, weight=1)
        editor.rowconfigure(4, weight=1)
        for row, label, variable in ((0, "Start", self.segment_start), (1, "Ende", self.segment_end)):
            ttk.Label(editor, text=label).grid(row=row, column=0, sticky="w")
            ttk.Entry(editor, textvariable=variable, width=12).grid(row=row, column=1, sticky="w")
        ttk.Label(editor, text="Sprecher:in").grid(row=2, column=0, sticky="w")
        self.segment_speaker_box = ttk.Combobox(editor, textvariable=self.segment_speaker, state="readonly")
        self.segment_speaker_box.grid(row=2, column=1, sticky="ew")
        ttk.Label(editor, text="Sprache").grid(row=3, column=0, sticky="w")
        ttk.Combobox(editor, textvariable=self.segment_language, values=("", "de", "en"), state="readonly").grid(row=3, column=1, sticky="ew")
        self.segment_text = Text(editor, wrap="word", height=12)
        self.segment_text.grid(row=4, column=0, columnspan=2, sticky="nsew", pady=8)
        self.segment_text.bind("<FocusOut>", lambda _event: self.save_selected_segment())
        buttons = ttk.Frame(editor)
        buttons.grid(row=5, column=0, columnspan=2, sticky="ew")
        for text, command in (("Übernehmen", self.save_selected_segment), ("Anhören", self.play_selected), ("Teilen", self.split_selected), ("Mit nächstem verbinden", self.merge_selected), ("Löschen", self.delete_selected)):
            ttk.Button(buttons, text=text, command=command).pack(side=LEFT, padx=(0, 5), pady=2)

    def _build_speakers(self, parent: ttk.Frame) -> None:
        parent.columnconfigure(0, weight=1)
        parent.rowconfigure(0, weight=1)
        self.speaker_tree = ttk.Treeview(parent, columns=("name", "duration"), show="tree headings", selectmode="browse")
        self.speaker_tree.heading("#0", text="Technische Kennung")
        self.speaker_tree.heading("name", text="Name")
        self.speaker_tree.heading("duration", text="Dauer")
        self.speaker_tree.grid(row=0, column=0, sticky="nsew")
        self.speaker_tree.bind("<<TreeviewSelect>>", lambda _event: self.load_speaker())
        actions = ttk.Frame(parent)
        actions.grid(row=1, column=0, sticky="ew", pady=(8, 0))
        self.speaker_name, self.merge_target = StringVar(), StringVar()
        ttk.Entry(actions, textvariable=self.speaker_name, width=28).pack(side=LEFT)
        ttk.Button(actions, text="Umbenennen", command=self.rename_speaker).pack(side=LEFT, padx=5)
        ttk.Button(actions, text="Hörprobe", command=self.play_speaker).pack(side=LEFT, padx=(0, 16))
        self.merge_box = ttk.Combobox(actions, textvariable=self.merge_target, state="readonly", width=20)
        self.merge_box.pack(side=LEFT)
        ttk.Button(actions, text="Zusammenführen", command=self.merge_speaker).pack(side=LEFT, padx=5)

    def _build_glossary(self, parent: ttk.Frame) -> None:
        parent.columnconfigure(0, weight=1)
        parent.rowconfigure(1, weight=1)
        actions = ttk.Frame(parent)
        actions.grid(row=0, column=0, sticky="ew", pady=(0, 8))
        ttk.Button(actions, text="Begriffe ermitteln", command=self.analyze_glossary).pack(side=LEFT)
        ttk.Button(actions, text="Ersetzungen anwenden", command=self.apply_glossary).pack(side=LEFT, padx=8)
        ttk.Label(actions, text="Ersatz für Auswahl:").pack(side=LEFT, padx=(20, 4))
        self.replacement = StringVar()
        ttk.Entry(actions, textvariable=self.replacement, width=32).pack(side=LEFT)
        self.glossary_tree = ttk.Treeview(parent, columns=("count", "kind", "replacement"), show="tree headings", selectmode="extended")
        for column, title in (("#0", "Begriff"), ("count", "Anzahl"), ("kind", "Typ"), ("replacement", "Ersetzung")):
            self.glossary_tree.heading(column, text=title)
        self.glossary_tree.grid(row=1, column=0, sticky="nsew")
        self.glossary_tree.bind("<<TreeviewSelect>>", lambda _event: self.load_replacement())
        ttk.Button(parent, text="Ersetzung für Auswahl setzen", command=self.set_replacement).grid(row=2, column=0, sticky="e", pady=(8, 0))
        self.candidates, self.replacements = [], {}

    def snapshot(self) -> None:
        self.undo_stack.append(copy.deepcopy(self.document))
        self.undo_stack = self.undo_stack[-50:]
        self.redo_stack.clear()
    def mutate(self, operation) -> None:
        self.snapshot()
        operation()
        self.persist(False)
        self.refresh_all()
    def undo(self) -> None:
        if self.undo_stack:
            self.redo_stack.append(copy.deepcopy(self.document))
            self.document = self.undo_stack.pop()
            self.persist(False)
            self.refresh_all()
    def redo(self) -> None:
        if self.redo_stack:
            self.undo_stack.append(copy.deepcopy(self.document))
            self.document = self.redo_stack.pop()
            self.persist(False)
            self.refresh_all()
    def refresh_all(self) -> None:
        labels = [""] + speaker_labels(self.document)
        self.bulk_box.configure(values=labels)
        self.segment_speaker_box.configure(values=labels)
        self.merge_box.configure(values=labels[1:])
        self.refresh_segments()
        self.refresh_speakers()
    def refresh_segments(self) -> None:
        selected = self.segment_tree.selection()
        self.segment_tree.delete(*self.segment_tree.get_children())
        query = self.search.get().casefold().strip()
        for segment in self.document.segments:
            speaker = self.document.speaker_names.get(segment.speaker or "", segment.speaker or "")
            if query and query not in segment.text.casefold() and query not in speaker.casefold():
                continue
            self.segment_tree.insert("", END, iid=segment.id, values=(f"{segment.start:.2f}–{segment.end:.2f}", speaker, (segment.language or "").upper(), segment.text))
        if selected and selected[0] in self.segment_tree.get_children():
            self.segment_tree.selection_set(selected[0])
    def selected_segment(self):
        selected = self.segment_tree.selection()
        return next((item for item in self.document.segments if selected and item.id == selected[0]), None)
    def load_segment(self) -> None:
        segment = self.selected_segment()
        if segment:
            self.segment_start.set(segment.start)
            self.segment_end.set(segment.end)
            self.segment_speaker.set(segment.speaker or "")
            self.segment_language.set(segment.language or "")
            self.segment_text.delete("1.0", END)
            self.segment_text.insert("1.0", segment.text)
    def save_selected_segment(self) -> None:
        segment = self.selected_segment()
        if not segment:
            return
        text = self.segment_text.get("1.0", END).strip()
        def operation() -> None:
            segment.start = max(0, self.segment_start.get())
            segment.end = max(segment.start, self.segment_end.get())
            segment.speaker = self.segment_speaker.get() or None
            segment.language = self.segment_language.get() or None
            segment.text = text
        self.mutate(operation)
    def split_selected(self) -> None:
        segment = self.selected_segment()
        if segment:
            self.mutate(lambda: split_segment(self.document, segment.id))
    def merge_selected(self) -> None:
        segment = self.selected_segment()
        if segment:
            self.mutate(lambda: merge_segment_with_next(self.document, segment.id))
    def delete_selected(self) -> None:
        segment = self.selected_segment()
        if segment and messagebox.askyesno(APP_TITLE, "Dieses Segment löschen?", parent=self):
            self.mutate(lambda: self.document.segments.remove(segment))
    def play_selected(self) -> None:
        segment = self.selected_segment()
        if segment:
            self.player.play(segment.start, segment.end)
    def bulk_assign(self, only_empty: bool) -> None:
        label = self.bulk_speaker.get() or None
        def operation() -> None:
            for segment in self.document.segments:
                if not only_empty or not segment.speaker:
                    segment.speaker = label
        self.mutate(operation)
    def refresh_speakers(self) -> None:
        self.speaker_tree.delete(*self.speaker_tree.get_children())
        for label in speaker_labels(self.document):
            duration = sum(max(0, item.end - item.start) for item in self.document.segments if item.speaker == label)
            self.speaker_tree.insert("", END, iid=label, text=label, values=(self.document.speaker_names.get(label, label), f"{duration:.1f} s"))
    def load_speaker(self) -> None:
        selected = self.speaker_tree.selection()
        if selected:
            self.speaker_name.set(self.document.speaker_names.get(selected[0], selected[0]))
    def rename_speaker(self) -> None:
        selected, name = self.speaker_tree.selection(), self.speaker_name.get().strip()
        if selected and name:
            self.mutate(lambda: self.document.speaker_names.__setitem__(selected[0], name))
    def merge_speaker(self) -> None:
        selected, target = self.speaker_tree.selection(), self.merge_target.get()
        if selected and target:
            self.mutate(lambda: merge_speakers(self.document, selected[0], target))
    def play_speaker(self) -> None:
        selected = self.speaker_tree.selection()
        if selected:
            segment = next((item for item in self.document.segments if item.speaker == selected[0] and item.end - item.start >= 0.4), None)
            if segment:
                self.player.play(segment.start, min(segment.end, segment.start + 8))
    def analyze_glossary(self) -> None:
        self.status.set("Begriffe werden ermittelt …")
        def run() -> None:
            try:
                values = glossary_candidates(self.document)
                self.after(0, lambda: self.show_candidates(values))
            except Exception as error:
                self.after(0, lambda value=str(error): self.status.set(f"Fehler: {value}"))
        threading.Thread(target=run, daemon=True).start()
    def show_candidates(self, values: list[dict]) -> None:
        self.candidates = values
        self.glossary_tree.delete(*self.glossary_tree.get_children())
        for index, item in enumerate(values):
            term = str(item.get("term") or "")
            self.glossary_tree.insert("", END, iid=str(index), text=term, values=(item.get("count", 0), item.get("kind", ""), self.replacements.get(term, "")))
        self.status.set(f"{len(values)} Begriffe ermittelt")
    def load_replacement(self) -> None:
        selected = self.glossary_tree.selection()
        if selected:
            term = self.glossary_tree.item(selected[0], "text")
            self.replacement.set(self.replacements.get(term, ""))
    def set_replacement(self) -> None:
        for selected in self.glossary_tree.selection():
            term, replacement = self.glossary_tree.item(selected, "text"), self.replacement.get().strip()
            if replacement:
                self.replacements[term] = replacement
            else:
                self.replacements.pop(term, None)
        self.show_candidates(self.candidates)
    def apply_glossary(self) -> None:
        if self.replacements:
            replacements = dict(self.replacements)
            def operation() -> None:
                replace_terms(self.document, replacements)
                self.app.store.save_glossary_terms(self.app.store.glossary_terms() + [value for key, value in replacements.items() if value != key])
            self.mutate(operation)
    def persist(self, show_status: bool) -> None:
        save_document(self.document, self.result.document_path, backup=True)
        if self.document.outputs:
            refresh_existing_outputs(self.document)
            save_document(self.document, self.result.document_path, backup=False)
        self.app.result_updated(self.result.document_path)
        if show_status:
            self.status.set("Gespeichert")
    def export(self) -> None:
        dialog = Toplevel(self)
        dialog.title("Exportieren")
        values = {name: BooleanVar(value=name in (self.document.outputs or {"markdown": ""})) for name in OUTPUT_FORMATS}
        ttk.Label(dialog, text="Ausgabeformate wählen", font=("Segoe UI", 11, "bold")).pack(anchor="w", padx=14, pady=(14, 8))
        for name, variable in values.items():
            ttk.Checkbutton(dialog, text="Markdown" if name == "markdown" else name.upper(), variable=variable).pack(anchor="w", padx=14)
        def finish() -> None:
            formats = {name for name, variable in values.items() if variable.get()}
            folder = filedialog.askdirectory(parent=dialog) if formats else ""
            if folder:
                self.snapshot()
                self.document.outputs = export_document(self.document, Path(folder), formats)
                self.persist(True)
                dialog.destroy()
        ttk.Button(dialog, text="Exportieren …", command=finish).pack(padx=14, pady=14)
    def cycle_rate(self) -> None:
        self.rate_label.set(f"{self.player.cycle_rate():g}×")
    def close(self) -> None:
        self.player.stop()
        self.persist(False)
        self.destroy()


def main() -> None:
    if "--smoke-test" in sys.argv:
        try:
            for tool in ("ffmpeg", "ffprobe", "ffplay", "deno"):
                if TOOLS.get(tool) is None:
                    raise RuntimeError(f"Bundled tool missing: {tool}")
            for module_name in ("torch", "whisper", "speechbrain", "silero_vad", "spacy", "yt_dlp", "keyring", "tkinterdnd2"):
                __import__(module_name)
            return
        except Exception:
            traceback.print_exc()
            raise SystemExit(1)
    try:
        from tkinterdnd2 import TkinterDnD
        root = TkinterDnD.Tk()
    except ImportError:
        root = Tk()
    TranscriptionApp(root)
    root.mainloop()


if __name__ == "__main__":
    main()
