#!/usr/bin/env python3
import math
import struct
import sys
import zlib
from pathlib import Path


ICON_SIZES = [16, 24, 32, 48, 64, 128, 256]


def write_png_bytes(width: int, height: int, pixels: bytearray) -> bytes:
    raw = bytearray()
    stride = width * 4
    for y in range(height):
        raw.append(0)
        raw.extend(pixels[y * stride:(y + 1) * stride])

    def chunk(kind: bytes, data: bytes) -> bytes:
        return (
            struct.pack(">I", len(data))
            + kind
            + data
            + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)
        )

    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(bytes(raw), 9))
        + chunk(b"IEND", b"")
    )


def arc(cx: float, cy: float, rx: float, ry: float, start_deg: float, end_deg: float, steps: int) -> list[tuple[float, float]]:
    start = math.radians(start_deg)
    end = math.radians(end_deg)
    return [
        (cx + math.cos(start + (end - start) * i / steps) * rx, cy + math.sin(start + (end - start) * i / steps) * ry)
        for i in range(steps + 1)
    ]


def blend_white(pixels: bytearray, size: int, x: int, y: int, alpha: float) -> None:
    if x < 0 or y < 0 or x >= size or y >= size:
        return
    value = max(0, min(255, int(round(alpha * 255))))
    offset = (y * size + x) * 4
    if value > pixels[offset]:
        pixels[offset:offset + 4] = bytes((value, value, value, 255))


def stamp_circle(pixels: bytearray, size: int, cx: float, cy: float, radius: float) -> None:
    min_x = int(math.floor(cx - radius - 1))
    max_x = int(math.ceil(cx + radius + 1))
    min_y = int(math.floor(cy - radius - 1))
    max_y = int(math.ceil(cy + radius + 1))
    for y in range(min_y, max_y + 1):
        for x in range(min_x, max_x + 1):
            distance = math.hypot((x + 0.5) - cx, (y + 0.5) - cy)
            alpha = radius + 0.75 - distance
            if alpha > 0:
                blend_white(pixels, size, x, y, min(1.0, alpha))


def draw_polyline(pixels: bytearray, size: int, points: list[tuple[float, float]], stroke_width: float) -> None:
    scale = size / 1024
    radius = stroke_width * scale / 2
    scaled = [(x * scale, y * scale) for x, y in points]
    for (ax, ay), (bx, by) in zip(scaled, scaled[1:]):
        length = max(1.0, math.hypot(bx - ax, by - ay))
        steps = max(1, int(math.ceil(length / max(1.0, radius * 0.55))))
        for step in range(steps + 1):
            t = step / steps
            stamp_circle(pixels, size, ax + (bx - ax) * t, ay + (by - ay) * t, radius)


def render(size: int) -> bytes:
    pixels = bytearray((0, 0, 0, 255) * size * size)
    stroke = 58
    shapes = [
        arc(512, 477, 274, 239, 213, 500, 190),
        [(343, 664), (317, 776), (438, 706)],
        arc(512, 526, 115, 82, 45, 135, 70),
        arc(730, 486, 110, 132, -55, 20, 50),
        arc(746, 482, 194, 224, -58, 17, 70),
    ]
    for shape in shapes:
        draw_polyline(pixels, size, shape, stroke)
    eye_radius = stroke * size / 1024 / 2
    for ex, ey in ((400, 459), (624, 459)):
        stamp_circle(pixels, size, ex * size / 1024, ey * size / 1024, eye_radius)
    return write_png_bytes(size, size, pixels)


def write_ico(path: Path, images: list[tuple[int, bytes]]) -> None:
    header = struct.pack("<HHH", 0, 1, len(images))
    directory = bytearray()
    data = bytearray()
    offset = 6 + 16 * len(images)
    for size, png in images:
        width = 0 if size == 256 else size
        height = 0 if size == 256 else size
        directory.extend(struct.pack("<BBBBHHII", width, height, 0, 0, 1, 32, len(png), offset))
        data.extend(png)
        offset += len(png)
    path.write_bytes(header + directory + data)


def main() -> int:
    if len(sys.argv) != 2:
        print("Usage: generate_windows_icon.py <output.ico>", file=sys.stderr)
        return 2
    output = Path(sys.argv[1])
    output.parent.mkdir(parents=True, exist_ok=True)
    write_ico(output, [(size, render(size)) for size in ICON_SIZES])
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
