import argparse
import os
import time
from datetime import datetime
from pathlib import Path

os.environ.setdefault("PYTORCH_ENABLE_MPS_FALLBACK", "1")

import torch
import whisper


FPS = 25


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


def load_whisper_model_robust(model_name: str, preferred_device: str):
    model = whisper.load_model(model_name, device="cpu")
    densify_sparse_buffers(model)

    if preferred_device == "mps":
        try:
            return model.to("mps"), "mps"
        except NotImplementedError:
            return model, "cpu"

    if preferred_device == "cuda":
        try:
            return model.to("cuda"), "cuda"
        except Exception:
            return model, "cpu"

    return model, "cpu"


def write_markdown(media_path: Path, output_dir: Path, model_name: str, device: str, include_timecodes: bool, result: dict) -> Path:
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

# Transkript: {media_stem}

---

{transcript_body}
"""
    output_path.write_text(markdown_content, encoding="utf-8")
    return output_path


def parse_args():
    parser = argparse.ArgumentParser(description="Bulk transcribe media files with OpenAI Whisper.")
    parser.add_argument("--file", action="append", required=True, help="Media file to transcribe. Repeat for bulk mode.")
    parser.add_argument("--language", choices=["de", "en"], required=True)
    parser.add_argument("--model", default="large")
    parser.add_argument("--buffer", type=int, default=2)
    parser.add_argument("--output-dir")
    parser.add_argument("--timecodes", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    files = [Path(file).resolve() for file in args.file]
    output_dir = Path(args.output_dir).resolve() if args.output_dir else None

    preferred = pick_preferred_device()
    print(f"Loading model '{args.model}' with preferred device '{preferred}' ...", flush=True)
    model, device = load_whisper_model_robust(args.model, preferred)
    print(f"Model loaded. Using device '{device}'.", flush=True)

    if device == "cpu":
        try:
            torch.set_num_threads(max(1, os.cpu_count() or 1))
        except Exception:
            pass

    for index, media_path in enumerate(files, start=1):
        print(f"Transcribing {index}/{len(files)}: {media_path.name}", flush=True)
        result = model.transcribe(
            str(media_path),
            language=args.language,
            verbose=False,
            fp16=False,
            beam_size=1,
            best_of=1,
            temperature=0,
            condition_on_previous_text=False,
        )

        target_dir = output_dir or media_path.parent
        target_dir.mkdir(parents=True, exist_ok=True)
        output_path = write_markdown(media_path, target_dir, args.model, device, args.timecodes, result)
        print(f"Saved: {output_path}", flush=True)

        if index < len(files) and args.buffer > 0:
            print(f"Waiting {args.buffer} seconds before the next file ...", flush=True)
            time.sleep(args.buffer)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
