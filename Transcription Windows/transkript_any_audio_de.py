import os
import time
from datetime import datetime
from tkinter import Tk, filedialog, simpledialog

os.environ.setdefault("PYTORCH_ENABLE_MPS_FALLBACK", "1")

import torch
import whisper


def seconds_to_timecode(seconds: float, fps: int = 25) -> str:
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
    for key, buf in list(module._buffers.items()):
        if torch.is_tensor(buf) and getattr(buf, "is_sparse", False):
            module._buffers[key] = buf.to_dense()
    for child in module.children():
        densify_sparse_buffers(child)


def pick_preferred_device() -> str:
    if torch.backends.mps.is_available():
        return "mps"
    if torch.cuda.is_available():
        return "cuda"
    return "cpu"


def load_whisper_model_robust(model_name: str, preferred_device: str) -> tuple[torch.nn.Module, str]:
    model = whisper.load_model(model_name, device="cpu")
    densify_sparse_buffers(model)

    if preferred_device == "mps":
        try:
            model = model.to("mps")
            return model, "mps"
        except NotImplementedError:
            return model, "cpu"

    if preferred_device == "cuda":
        try:
            model = model.to("cuda")
            return model, "cuda"
        except Exception:
            return model, "cpu"

    return model, "cpu"


root = Tk()
root.withdraw()
media_paths = filedialog.askopenfilenames(
    title="Wähle eine oder mehrere Audio- oder Videodateien zur Transkription aus",
    filetypes=[
        (
            "Audio and Video Files",
            "*.wav *.mp3 *.m4a *.flac *.aac *.ogg *.wma "
            "*.mp4 *.mov *.MOV *.m4v *.avi *.mkv *.webm",
        ),
        ("Audio Files", "*.wav *.mp3 *.m4a *.flac *.aac *.ogg *.wma"),
        ("Video Files", "*.mp4 *.mov *.MOV *.m4v *.avi *.mkv *.webm"),
        ("All Files", "*.*"),
    ],
)

if not media_paths:
    print("Keine Datei(en) ausgewaehlt. Vorgang abgebrochen.")
    raise SystemExit(0)

FPS = 25
BULK_FILE_BUFFER_SECONDS = 2
DEFAULT_MODEL_NAME = "large"
MODEL_NAME = simpledialog.askstring(
    "Whisper Modell",
    "Whisper Modell auswaehlen (tiny, base, small, medium, large):",
    initialvalue=DEFAULT_MODEL_NAME,
    parent=root,
)
MODEL_NAME = (MODEL_NAME or DEFAULT_MODEL_NAME).strip() or DEFAULT_MODEL_NAME

preferred = pick_preferred_device()
print(f"Lade Modell ({MODEL_NAME}) mit bevorzugtem device='{preferred}' ...")
model, device = load_whisper_model_robust(MODEL_NAME, preferred)
print(f"Modell geladen. Verwendetes device='{device}'")

if device == "cpu":
    try:
        torch.set_num_threads(max(1, os.cpu_count() or 1))
    except Exception:
        pass

for index, media_path in enumerate(media_paths, start=1):
    media_name = os.path.basename(media_path)
    media_stem = os.path.splitext(media_name)[0]
    output_path = os.path.join(os.path.dirname(media_path), f"{media_stem}.md")

    print(f"\nTranskription {index}/{len(media_paths)}: {media_name}")
    result = model.transcribe(
        media_path,
        language="de",
        verbose=False,
        fp16=False,                  # fuer macOS MPS/CPU stabil
        beam_size=1,
        best_of=1,
        temperature=0,
        condition_on_previous_text=False
    )

    segments = result.get("segments", [])
    full_text = (result.get("text") or "").strip()

    segment_blocks = []
    for seg in segments:
        start_tc = seconds_to_timecode(float(seg.get("start", 0.0)), fps=FPS)
        end_tc = seconds_to_timecode(float(seg.get("end", 0.0)), fps=FPS)
        seg_text = (seg.get("text") or "").strip()
        if seg_text:
            segment_blocks.append(f"{start_tc} - {end_tc}\n{seg_text}")

    timestamp = datetime.now().strftime("%Y-%m-%d %H:%M")
    timed_transcript = "\n\n".join(segment_blocks).strip() if segment_blocks else full_text

    markdown_content = f"""\
---
created: {yaml_quote(timestamp)}
model: {yaml_quote(MODEL_NAME)}
device: {yaml_quote(device)}
source_file: {yaml_quote(media_name)}
fps_timecode: {FPS}
---

# Transkript: {media_stem}

---

{timed_transcript}
"""

    with open(output_path, "w", encoding="utf-8") as f:
        f.write(markdown_content)

    print(f"Transkript gespeichert unter: {output_path}")

    if index < len(media_paths):
        print(f"Warte {BULK_FILE_BUFFER_SECONDS} Sekunden vor der naechsten Datei ...")
        time.sleep(BULK_FILE_BUFFER_SECONDS)
