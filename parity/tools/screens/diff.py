#!/usr/bin/env python3
"""두 PNG의 픽셀 차이 비율을 계산한다.

Pillow가 있으면 Pillow로 읽고, 없으면 zlib만으로 8비트 RGB/RGBA PNG를 직접 디코드한다.
한 채널이라도 값이 다르면 그 픽셀을 다른 픽셀로 센다(허용 오차 없음). 크기가 다르면 비율 1.0으로 본다.

사용법:
  python3 diff.py a.png b.png [--diff-out diff.png]
  python3 diff.py --dirs baseline/ candidate/ [--threshold 0.02]
"""
from __future__ import annotations

import argparse
import json
import struct
import sys
import zlib
from pathlib import Path

try:
    from PIL import Image, ImageChops

    HAS_PIL = True
except ImportError:  # pragma: no cover - Pillow가 없는 환경용
    HAS_PIL = False


def paeth_predictor(left: int, up: int, up_left: int) -> int:
    estimate = left + up - up_left
    distance_left, distance_up, distance_up_left = abs(estimate - left), abs(estimate - up), abs(estimate - up_left)
    if distance_left <= distance_up and distance_left <= distance_up_left:
        return left
    if distance_up <= distance_up_left:
        return up
    return up_left


def decode_png_pure(path: Path) -> tuple[int, int, list[bytes]]:
    """8비트 RGB(2)·RGBA(6) PNG만 지원한다. 행마다 RGB 바이트를 돌려준다."""
    data = path.read_bytes()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError(f"not a PNG: {path}")
    offset, idat, width = 8, b"", 0
    height = bit_depth = color_type = 0
    while offset < len(data):
        (length,) = struct.unpack(">I", data[offset:offset + 4])
        kind = data[offset + 4:offset + 8]
        body = data[offset + 8:offset + 8 + length]
        offset += 12 + length
        if kind == b"IHDR":
            width, height, bit_depth, color_type = struct.unpack(">IIBB", body[:10])
        elif kind == b"IDAT":
            idat += body
        elif kind == b"IEND":
            break
    if bit_depth != 8 or color_type not in (2, 6):
        raise ValueError(f"unsupported PNG format (depth {bit_depth}, color {color_type}): {path}")
    channels = 3 if color_type == 2 else 4
    stride = width * channels
    raw = zlib.decompress(idat)
    rows, previous = [], bytearray(stride)
    position = 0
    for _ in range(height):
        filter_type = raw[position]
        line = bytearray(raw[position + 1:position + 1 + stride])
        position += 1 + stride
        for index in range(stride):
            left = line[index - channels] if index >= channels else 0
            up = previous[index]
            up_left = previous[index - channels] if index >= channels else 0
            if filter_type == 1:
                line[index] = (line[index] + left) & 0xFF
            elif filter_type == 2:
                line[index] = (line[index] + up) & 0xFF
            elif filter_type == 3:
                line[index] = (line[index] + ((left + up) >> 1)) & 0xFF
            elif filter_type == 4:
                line[index] = (line[index] + paeth_predictor(left, up, up_left)) & 0xFF
        previous = line
        if channels == 4:
            rows.append(bytes(byte for pixel in range(width) for byte in line[pixel * 4:pixel * 4 + 3]))
        else:
            rows.append(bytes(line))
    return width, height, rows


def compare_images(first_path: Path, second_path: Path, diff_output: Path | None = None) -> dict:
    if HAS_PIL:
        first = Image.open(first_path).convert("RGB")
        second = Image.open(second_path).convert("RGB")
        if first.size != second.size:
            return {"diff_pixels": None, "total_pixels": None, "diff_ratio": 1.0, "size_mismatch": True}
        difference = ImageChops.difference(first, second)
        mask = difference.convert("L").point(lambda value: 255 if value else 0)
        diff_pixels = mask.histogram()[255]
        total_pixels = first.size[0] * first.size[1]
        if diff_output is not None:
            highlighted = first.convert("L").convert("RGB")
            highlighted.paste((255, 0, 64), mask=mask)
            highlighted.save(diff_output)
    else:
        first_width, first_height, first_rows = decode_png_pure(first_path)
        second_width, second_height, second_rows = decode_png_pure(second_path)
        if (first_width, first_height) != (second_width, second_height):
            return {"diff_pixels": None, "total_pixels": None, "diff_ratio": 1.0, "size_mismatch": True}
        diff_pixels = 0
        for first_row, second_row in zip(first_rows, second_rows):
            if first_row == second_row:
                continue
            for pixel in range(first_width):
                if first_row[pixel * 3:pixel * 3 + 3] != second_row[pixel * 3:pixel * 3 + 3]:
                    diff_pixels += 1
        total_pixels = first_width * first_height
    return {
        "diff_pixels": diff_pixels,
        "total_pixels": total_pixels,
        "diff_ratio": round(diff_pixels / total_pixels, 8),
        "size_mismatch": False,
    }


def compare_directories(baseline_dir: Path, candidate_dir: Path, threshold: float) -> int:
    results, failures = {}, 0
    for baseline_path in sorted(baseline_dir.glob("*.png")):
        candidate_path = candidate_dir / baseline_path.name
        if not candidate_path.exists():
            results[baseline_path.stem] = {"missing": True, "pass": False}
            failures += 1
            continue
        comparison = compare_images(baseline_path, candidate_path)
        comparison["pass"] = comparison["diff_ratio"] <= threshold
        failures += 0 if comparison["pass"] else 1
        results[baseline_path.stem] = comparison
    print(json.dumps({"threshold": threshold, "failures": failures, "results": results}, indent=2))
    return 1 if failures else 0


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("paths", nargs="*")
    parser.add_argument("--dirs", nargs=2, metavar=("BASELINE", "CANDIDATE"))
    parser.add_argument("--threshold", type=float, default=0.02)
    parser.add_argument("--diff-out")
    args = parser.parse_args()
    if args.dirs:
        sys.exit(compare_directories(Path(args.dirs[0]), Path(args.dirs[1]), args.threshold))
    if len(args.paths) != 2:
        parser.error("two PNG paths or --dirs are required")
    result = compare_images(Path(args.paths[0]), Path(args.paths[1]), Path(args.diff_out) if args.diff_out else None)
    print(json.dumps(result))
    sys.exit(0 if result["diff_ratio"] <= args.threshold else 1)


if __name__ == "__main__":
    main()
