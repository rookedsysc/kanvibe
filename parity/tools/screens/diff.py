#!/usr/bin/env python3
"""두 PNG의 픽셀 차이 비율을 계산한다.

Pillow가 있으면 Pillow로 읽고, 없으면 zlib만으로 8비트 RGB/RGBA PNG를 직접 디코드한다. 크기가 다르면 비율 1.0으로 본다.

기본 판정은 pixelmatch(mapbox/pixelmatch)의 픽셀별 YIQ 색 차이다. 두 픽셀의 YIQ 거리 제곱이
`35215 * threshold²`(기본 threshold 0.1)를 넘을 때만 다른 픽셀로 센다. pixelmatch의 안티에일리어싱 판별은 쓰지 않는다.
GPUI와 Chromium의 글자 안티에일리어싱이 1~20단계 달라 엄격 비교로는 글자 많은 화면이 2%를 넘기 때문이다(계획 Change Log).
`--strict`는 한 채널이라도 값이 다르면 다른 픽셀로 센다. 결과 JSON에는 두 비율을 모두 싣고 `diff_ratio`가 판정에 쓴 쪽이다.

사용법:
  python3 diff.py a.png b.png [--diff-out diff.png] [--strict] [--pixel-threshold 0.1]
  python3 diff.py --dirs baseline/ candidate/ [--threshold 0.02] [--strict]
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

# pixelmatch 기본값과 같은 픽셀 허용치(0~1). 값이 작을수록 엄격하다
DEFAULT_PIXEL_THRESHOLD = 0.1
# pixelmatch가 정한 YIQ 공간의 최대 거리 제곱. 허용 거리 제곱은 이 값 × threshold²이다
MAX_YIQ_DELTA = 35215

# 차이 그림 색: 허용치를 넘은 픽셀(빨강)과 값은 다르지만 허용치 안인 픽셀(노랑)
OVER_TOLERANCE_COLOR = (255, 0, 64)
WITHIN_TOLERANCE_COLOR = (255, 200, 0)


def yiq_color_delta(first: tuple[int, int, int], second: tuple[int, int, int]) -> float:
    """pixelmatch `colorDelta`를 불투명 RGB 픽셀에 적용한 값. 첫 픽셀이 더 밝으면 음수다."""
    delta_red = first[0] - second[0]
    delta_green = first[1] - second[1]
    delta_blue = first[2] - second[2]
    if not (delta_red or delta_green or delta_blue):
        return 0.0
    luma = delta_red * 0.29889531 + delta_green * 0.58662247 + delta_blue * 0.11448223
    in_phase = delta_red * 0.59597799 - delta_green * 0.27417610 - delta_blue * 0.32180189
    quadrature = delta_red * 0.21147017 - delta_green * 0.52261711 + delta_blue * 0.31114694
    delta = 0.5053 * luma * luma + 0.299 * in_phase * in_phase + 0.1957 * quadrature * quadrature
    return -delta if luma > 0 else delta


def max_allowed_delta(pixel_threshold: float) -> float:
    return MAX_YIQ_DELTA * pixel_threshold * pixel_threshold


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


class RowPixels:
    """순수 Python 디코더의 행 바이트를 Pillow `load()`처럼 `(x, y)`로 읽는다"""

    def __init__(self, rows: list[bytes]):
        self.rows = rows

    def __getitem__(self, position: tuple[int, int]) -> tuple[int, int, int]:
        column, row = position
        return tuple(self.rows[row][column * 3:column * 3 + 3])


def find_strictly_different_pixels_pil(first: "Image.Image", second: "Image.Image") -> list[int]:
    """한 채널이라도 다른 픽셀의 평면 인덱스. 허용치 계산은 이 픽셀들에만 하면 된다."""
    mask = ImageChops.difference(first, second).convert("L").point(lambda value: 255 if value else 0)
    # Pillow 12부터 getdata()는 폐기 예정이다. 이전 버전에는 get_flattened_data()가 없다
    flattened = mask.get_flattened_data() if hasattr(mask, "get_flattened_data") else mask.getdata()
    return [index for index, value in enumerate(flattened) if value]


def compare_images(first_path: Path, second_path: Path, diff_output: Path | None = None, *,
                   strict: bool = False, pixel_threshold: float = DEFAULT_PIXEL_THRESHOLD) -> dict:
    allowed_delta = max_allowed_delta(pixel_threshold)
    if HAS_PIL:
        first = Image.open(first_path).convert("RGB")
        second = Image.open(second_path).convert("RGB")
        if first.size != second.size:
            return size_mismatch_result(strict, pixel_threshold)
        width, height = first.size
        first_pixels, second_pixels = first.load(), second.load()
        strict_indices = find_strictly_different_pixels_pil(first, second)
    else:
        width, height, first_rows = decode_png_pure(first_path)
        second_width, second_height, second_rows = decode_png_pure(second_path)
        if (width, height) != (second_width, second_height):
            return size_mismatch_result(strict, pixel_threshold)
        first_pixels = RowPixels(first_rows)
        second_pixels = RowPixels(second_rows)
        strict_indices = [row * width + column
                          for row, (first_row, second_row) in enumerate(zip(first_rows, second_rows))
                          if first_row != second_row
                          for column in range(width)
                          if first_row[column * 3:column * 3 + 3] != second_row[column * 3:column * 3 + 3]]

    over_tolerance = set()
    for index in strict_indices:
        position = (index % width, index // width)
        if abs(yiq_color_delta(first_pixels[position], second_pixels[position])) > allowed_delta:
            over_tolerance.add(index)

    if diff_output is not None and HAS_PIL:
        highlighted = first.convert("L").convert("RGB")
        highlighted_pixels = highlighted.load()
        for index in strict_indices:
            position = (index % width, index // width)
            highlighted_pixels[position] = OVER_TOLERANCE_COLOR if index in over_tolerance else WITHIN_TOLERANCE_COLOR
        highlighted.save(diff_output)

    total_pixels = width * height
    strict_ratio = round(len(strict_indices) / total_pixels, 8)
    tolerant_ratio = round(len(over_tolerance) / total_pixels, 8)
    judged_pixels = len(strict_indices) if strict else len(over_tolerance)
    return {
        "mode": "strict" if strict else "pixelmatch",
        "pixel_threshold": pixel_threshold,
        "diff_pixels": judged_pixels,
        "total_pixels": total_pixels,
        "diff_ratio": strict_ratio if strict else tolerant_ratio,
        "strict_diff_pixels": len(strict_indices),
        "strict_diff_ratio": strict_ratio,
        "tolerant_diff_pixels": len(over_tolerance),
        "tolerant_diff_ratio": tolerant_ratio,
        "size_mismatch": False,
    }


def size_mismatch_result(strict: bool, pixel_threshold: float) -> dict:
    return {
        "mode": "strict" if strict else "pixelmatch",
        "pixel_threshold": pixel_threshold,
        "diff_pixels": None,
        "total_pixels": None,
        "diff_ratio": 1.0,
        "strict_diff_ratio": 1.0,
        "tolerant_diff_ratio": 1.0,
        "size_mismatch": True,
    }


def compare_directories(baseline_dir: Path, candidate_dir: Path, threshold: float, *,
                        strict: bool, pixel_threshold: float) -> int:
    results, failures = {}, 0
    for baseline_path in sorted(baseline_dir.glob("*.png")):
        candidate_path = candidate_dir / baseline_path.name
        if not candidate_path.exists():
            results[baseline_path.stem] = {"missing": True, "pass": False}
            failures += 1
            continue
        comparison = compare_images(baseline_path, candidate_path, strict=strict, pixel_threshold=pixel_threshold)
        comparison["pass"] = comparison["diff_ratio"] <= threshold
        failures += 0 if comparison["pass"] else 1
        results[baseline_path.stem] = comparison
    print(json.dumps({"threshold": threshold, "mode": "strict" if strict else "pixelmatch",
                      "pixel_threshold": pixel_threshold, "failures": failures, "results": results}, indent=2))
    return 1 if failures else 0


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("paths", nargs="*")
    parser.add_argument("--dirs", nargs=2, metavar=("BASELINE", "CANDIDATE"))
    parser.add_argument("--threshold", type=float, default=0.02, help="이미지 하나에서 허용하는 다른 픽셀 비율")
    parser.add_argument("--pixel-threshold", type=float, default=DEFAULT_PIXEL_THRESHOLD,
                        help="pixelmatch 픽셀 허용치(0~1, 기본 0.1)")
    parser.add_argument("--strict", action="store_true", help="허용치 없이 값이 하나라도 다르면 다른 픽셀로 센다")
    parser.add_argument("--diff-out")
    args = parser.parse_args()
    if args.dirs:
        sys.exit(compare_directories(Path(args.dirs[0]), Path(args.dirs[1]), args.threshold,
                                     strict=args.strict, pixel_threshold=args.pixel_threshold))
    if len(args.paths) != 2:
        parser.error("two PNG paths or --dirs are required")
    result = compare_images(Path(args.paths[0]), Path(args.paths[1]), Path(args.diff_out) if args.diff_out else None,
                            strict=args.strict, pixel_threshold=args.pixel_threshold)
    print(json.dumps(result))
    sys.exit(0 if result["diff_ratio"] <= args.threshold else 1)


if __name__ == "__main__":
    main()
