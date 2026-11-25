#!/usr/bin/env python3
"""GPUI 앱이 번들하는 글꼴 파일을 기준 이미지의 글꼴 파일에서 재현 가능하게 만든다.

왜 원본을 그대로 넣지 않는가:
  gpui-pre-linux의 CosmicTextSystem은 시스템 글꼴을 먼저 읽고 번들 글꼴을 그 뒤에 더한다. family 이름이 같으면
  family 조회·굵기 매칭 모두 먼저 읽힌 시스템 얼굴을 고른다. 그래서 번들 얼굴만 쓰도록 모든 파일의 family 이름에
  `KanVibe ` 접두어를 붙인다(name 표만 바꾸고 글리프·메트릭 표는 바이트 그대로 둔다). TTC는 KR 얼굴만 떼어 낸다.

한글 합성 bold:
  기준 이미지의 굵은 한글은 Chromium(Skia)이 Noto Sans CJK KR Regular 윤곽선을 FreeType `FT_Outline_Embolden`으로
  굵게 만든 것이다. Skia의 세기는 `units_per_EM × y_scale / 24`, 즉 글자 크기의 1/24이고 advance는 바꾸지 않는다.
  같은 연산을 글꼴 단위에서 미리 해 둔다: 시스템 libfreetype을 ctypes로 불러 각 글리프를 FT_LOAD_NO_SCALE로 읽고,
  정밀도를 위해 64배로 늘린 뒤 세기 `upem × 64 / 24`로 굵게 하고 다시 줄여 정수로 반올림한다. 결과 윤곽선은
  삼차 곡선 그대로 CFF 표에 쓰고(힌트 없음), advance는 원본 hmtx 값을 쓴다. OS/2 굵기 700, family는
  `KanVibe Noto Sans CJK KR Synthetic Bold`로 따로 등록해 굵은 글씨 대체 순서에만 넣는다.
  대상 글자는 한글 블록, CJK 기호·전각 문자, ASCII(GPUI는 'm'이 없는 얼굴을 대체 글꼴에서 빼기 때문), 그리고
  `messages/*.json`에서 Noto Sans Bold에 없고 KR 얼굴에 있는 글자다.

라이선스: Noto(OFL 1.1, 예약 이름 없음)·DejaVu(Bitstream Vera 계열, 수정본은 'Bitstream'·'Vera'를 이름에 쓰지 않음)
  모두 이름을 바꾼 수정본 배포를 허용한다. 원본 라이선스 파일은 `assets/fonts/LICENSE-*.txt`에 그대로 둔다.

실행(fontTools 없이 Python 표준 라이브러리 + libfreetype.so.6만 쓴다):
  python3 parity/tools/fonts/build_app_fonts.py [--out crates/kanvibe-app/assets/fonts]
"""
from __future__ import annotations

import argparse
import ctypes
import hashlib
import json
import math
import struct
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[3]
DEFAULT_OUTPUT_DIR = REPO_ROOT / "crates/kanvibe-app/assets/fonts"
MESSAGES_DIR = REPO_ROOT / "messages"

FAMILY_PREFIX = "KanVibe "
POSTSCRIPT_PREFIX = "KanVibe-"
SYNTHETIC_BOLD_FAMILY = "KanVibe Noto Sans CJK KR Synthetic Bold"
SYNTHETIC_BOLD_POSTSCRIPT = "KanVibe-NotoSansCJKkr-SyntheticBold"
CJK_FACE_FAMILY = "Noto Sans CJK KR"
# Skia SkFontHost_FreeType_common.cpp `kOutlineEmboldenDivisor`
EMBOLDEN_DIVISOR = 24
# FT_Outline_Embolden 정밀도를 올리려고 글꼴 단위를 26.6 고정소수처럼 늘리는 배율
OUTLINE_PRECISION = 64

# (기준 환경 경로, sha256, 출력 파일). parity/screens/fonts.json·fonts.md와 같은 파일이다
SOURCE_FONTS = [
    ("/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
     "89c3c497f618fdaa0b2d1e98fef93582f28c71debd2c4a8cdf41f190ced2909d", "KanVibeNotoSans-Regular.ttf"),
    ("/usr/share/fonts/truetype/noto/NotoSans-Bold.ttf",
     "e83493c945848ecd4a9ad0f6d19164541a0d3e23a9c952304a00a46e00272ac5", "KanVibeNotoSans-Bold.ttf"),
    # Mermaid class·C4 등의 이탤릭 글자. Chromium은 `font-style: italic`을 fontconfig로 이 파일에 풀었다
    ("/usr/share/fonts/truetype/noto/NotoSans-Italic.ttf",
     "5d7618dda819443fd07301c84adcee9fe8960c873ea5e5e1f0f59db384b78082", "KanVibeNotoSans-Italic.ttf"),
    ("/usr/share/fonts/truetype/noto/NotoSans-BoldItalic.ttf",
     "4315b26e5afb62f70f5ffe0edaa3363e57765ff2af2beda12b6fef197e3e5f5a", "KanVibeNotoSans-BoldItalic.ttf"),
    ("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
     "b76b0433203017ca80401b2ee0dd69350349871c4b19d504c34dbdd80541690a", "KanVibeNotoSansCJKkr-Regular.otf"),
    ("/usr/share/fonts/truetype/noto/NotoSansSymbols-Regular.ttf",
     "24b615ceb7d50a2c09060664ca9e24a160c9beb6bfca0563e8359369a4bc0993", "KanVibeNotoSansSymbols-Regular.ttf"),
    ("/usr/share/fonts/truetype/noto/NotoSansSymbols2-Regular.ttf",
     "876598db5f1e3bef2bc77c4d94c5b0d5d2c1620be4878a06f18fb5434e331688", "KanVibeNotoSansSymbols2-Regular.ttf"),
    ("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
     "b4c632e3cdf9acc7f28758fb5a323c8524d7fc6660d46904d9b6cbe2809c419c", "KanVibeDejaVuSans.ttf"),
    ("/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
     "a54dca07c76d6289e717e75e0a58c0128f6d7269ef3faf76417c9d7d3bba37ab", "KanVibeDejaVuSansMono.ttf"),
    ("/usr/share/fonts/truetype/dejavu/DejaVuSansMono-Bold.ttf",
     "2d67eed9b325ee2e69f906ce4f3f6264d5105230a13352c8494daad4f17b12c8", "KanVibeDejaVuSansMono-Bold.ttf"),
]
CJK_SOURCE_PATH = "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc"
LATIN_BOLD_SOURCE_PATH = "/usr/share/fonts/truetype/noto/NotoSans-Bold.ttf"
SYNTHETIC_BOLD_OUTPUT = "KanVibeNotoSansCJKkr-SyntheticBold.otf"

SYNTHETIC_BOLD_RANGES = [
    (0x0020, 0x007E),  # ASCII
    (0x1100, 0x11FF),  # Hangul Jamo
    (0x3000, 0x303F),  # CJK Symbols and Punctuation
    (0x3130, 0x318F),  # Hangul Compatibility Jamo
    (0xA960, 0xA97F),  # Hangul Jamo Extended-A
    (0xAC00, 0xD7A3),  # Hangul Syllables
    (0xD7B0, 0xD7FF),  # Hangul Jamo Extended-B
    (0xFF00, 0xFFEF),  # Halfwidth and Fullwidth Forms
]

# name 표에서 family를 나타내는 레코드(family, unique ID, full name, PostScript, typographic family/subfamily)
RENAMED_NAME_IDS = {1, 3, 4, 6, 16, 17}
WINDOWS_ENGLISH = (3, 1, 0x409)


# ───────────── sfnt 읽기·쓰기 ─────────────

def read_verified(path: str, expected_sha256: str) -> bytes:
    data = Path(path).read_bytes()
    actual = hashlib.sha256(data).hexdigest()
    if actual != expected_sha256:
        raise SystemExit(f"{path}: sha256 {actual} != {expected_sha256} (기준 이미지와 다른 글꼴 버전)")
    return data


def face_offsets(data: bytes) -> list[int]:
    if data[:4] != b"ttcf":
        return [0]
    (face_count,) = struct.unpack(">I", data[8:12])
    return list(struct.unpack(f">{face_count}I", data[12:12 + 4 * face_count]))


def read_tables(data: bytes, face_offset: int) -> tuple[int, dict[str, bytes]]:
    sfnt_version, table_count = struct.unpack(">IH", data[face_offset:face_offset + 6])
    tables = {}
    for index in range(table_count):
        entry = face_offset + 12 + 16 * index
        tag, _, offset, length = struct.unpack(">4sIII", data[entry:entry + 16])
        tables[tag.decode("latin-1")] = data[offset:offset + length]
    return sfnt_version, tables


def table_checksum(table: bytes) -> int:
    padded = table + b"\0" * (-len(table) % 4)
    return sum(struct.unpack(f">{len(padded) // 4}I", padded)) & 0xFFFFFFFF


def write_sfnt(sfnt_version: int, tables: dict[str, bytes]) -> bytes:
    tables = dict(tables)
    tables["head"] = tables["head"][:8] + b"\0\0\0\0" + tables["head"][12:]
    tags = sorted(tables)
    entry_selector = int(math.log2(len(tags)))
    search_range = 16 * (1 << entry_selector)
    header = struct.pack(">IHHHH", sfnt_version, len(tags), search_range, entry_selector,
                         len(tags) * 16 - search_range)
    offset = len(header) + 16 * len(tags)
    directory, body = b"", b""
    for tag in tags:
        table = tables[tag]
        directory += struct.pack(">4sIII", tag.encode("latin-1"), table_checksum(table), offset + len(body), len(table))
        body += table + b"\0" * (-len(table) % 4)
    font = bytearray(header + directory + body)
    head_offset = offset + sum(len(tables[tag]) + (-len(tables[tag]) % 4) for tag in tags[:tags.index("head")])
    adjustment = (0xB1B0AFBA - table_checksum(bytes(font))) & 0xFFFFFFFF
    font[head_offset + 8:head_offset + 12] = struct.pack(">I", adjustment)
    return bytes(font)


# ───────────── name 표 ─────────────

def decode_name(platform_id: int, raw: bytes) -> str:
    return raw.decode("utf-16-be") if platform_id in (0, 3) else raw.decode("latin-1")


def encode_name(platform_id: int, text: str) -> bytes:
    return text.encode("utf-16-be") if platform_id in (0, 3) else text.encode("latin-1")


def read_name_records(name_table: bytes) -> list[tuple[int, int, int, int, str]]:
    _, count, string_offset = struct.unpack(">HHH", name_table[:6])
    records = []
    for index in range(count):
        platform_id, encoding_id, language_id, name_id, length, offset = struct.unpack(
            ">6H", name_table[6 + 12 * index:18 + 12 * index])
        raw = name_table[string_offset + offset:string_offset + offset + length]
        records.append((platform_id, encoding_id, language_id, name_id, decode_name(platform_id, raw)))
    return records


def english_name(records, name_id: int) -> str | None:
    for platform_id, encoding_id, language_id, record_name_id, text in records:
        if (platform_id, encoding_id, language_id) == WINDOWS_ENGLISH and record_name_id == name_id:
            return text
    return None


def build_name_table(records) -> bytes:
    records = sorted(records, key=lambda record: record[:4])
    header_size = 6 + 12 * len(records)
    entries, strings = b"", b""
    for platform_id, encoding_id, language_id, name_id, text in records:
        raw = encode_name(platform_id, text)
        entries += struct.pack(">6H", platform_id, encoding_id, language_id, name_id, len(raw), len(strings))
        strings += raw
    return struct.pack(">HHH", 0, len(records), header_size) + entries + strings


def is_english_record(platform_id: int, language_id: int) -> bool:
    return platform_id == 0 or (platform_id == 3 and language_id == 0x409) or (platform_id == 1 and language_id == 0)


def prefixed_name(name_id: int, text: str) -> str:
    if name_id == 6:
        return POSTSCRIPT_PREFIX + text
    if name_id == 17:
        return text
    return FAMILY_PREFIX + text


def rename_records(records):
    """family 계열 레코드에 접두어를 붙이고, 원래 이름으로 매칭될 수 있는 다른 언어 레코드는 버린다"""
    renamed = []
    for platform_id, encoding_id, language_id, name_id, text in records:
        if name_id not in RENAMED_NAME_IDS:
            renamed.append((platform_id, encoding_id, language_id, name_id, text))
        elif is_english_record(platform_id, language_id):
            renamed.append((platform_id, encoding_id, language_id, name_id, prefixed_name(name_id, text)))
    return renamed


def find_face(data: bytes, family: str) -> tuple[int, int]:
    for face_index, offset in enumerate(face_offsets(data)):
        _, tables = read_tables(data, offset)
        if english_name(read_name_records(tables["name"]), 1) == family:
            return face_index, offset
    raise SystemExit(f"face {family!r} not found")


def renamed_font(data: bytes, face_offset: int) -> bytes:
    sfnt_version, tables = read_tables(data, face_offset)
    tables["name"] = build_name_table(rename_records(read_name_records(tables["name"])))
    # TTC 전용 서명 표는 떼어 낸 단일 얼굴에서 의미가 없다
    tables.pop("DSIG", None)
    return write_sfnt(sfnt_version, tables)


# ───────────── FreeType(ctypes) ─────────────

class FtVector(ctypes.Structure):
    _fields_ = [("x", ctypes.c_long), ("y", ctypes.c_long)]


class FtMatrix(ctypes.Structure):
    _fields_ = [("xx", ctypes.c_long), ("xy", ctypes.c_long), ("yx", ctypes.c_long), ("yy", ctypes.c_long)]


class FtGeneric(ctypes.Structure):
    _fields_ = [("data", ctypes.c_void_p), ("finalizer", ctypes.c_void_p)]


class FtOutline(ctypes.Structure):
    # FreeType 2.13.3부터 n_contours·n_points·contours가 unsigned short다(크기는 그대로)
    _fields_ = [("n_contours", ctypes.c_ushort), ("n_points", ctypes.c_ushort),
                ("points", ctypes.POINTER(FtVector)), ("tags", ctypes.POINTER(ctypes.c_ubyte)),
                ("contours", ctypes.POINTER(ctypes.c_ushort)), ("flags", ctypes.c_int)]


class FtGlyphMetrics(ctypes.Structure):
    _fields_ = [(name, ctypes.c_long) for name in (
        "width", "height", "horiBearingX", "horiBearingY", "horiAdvance",
        "vertBearingX", "vertBearingY", "vertAdvance")]


class FtBitmap(ctypes.Structure):
    _fields_ = [("rows", ctypes.c_uint), ("width", ctypes.c_uint), ("pitch", ctypes.c_int),
                ("buffer", ctypes.c_void_p), ("num_grays", ctypes.c_ushort), ("pixel_mode", ctypes.c_ubyte),
                ("palette_mode", ctypes.c_ubyte), ("palette", ctypes.c_void_p)]


class FtGlyphSlot(ctypes.Structure):
    _fields_ = [("library", ctypes.c_void_p), ("face", ctypes.c_void_p), ("next", ctypes.c_void_p),
                ("glyph_index", ctypes.c_uint), ("generic", FtGeneric), ("metrics", FtGlyphMetrics),
                ("linearHoriAdvance", ctypes.c_long), ("linearVertAdvance", ctypes.c_long),
                ("advance", FtVector), ("format", ctypes.c_uint), ("bitmap", FtBitmap),
                ("bitmap_left", ctypes.c_int), ("bitmap_top", ctypes.c_int), ("outline", FtOutline)]


class FtFace(ctypes.Structure):
    _fields_ = [("num_faces", ctypes.c_long), ("face_index", ctypes.c_long), ("face_flags", ctypes.c_long),
                ("style_flags", ctypes.c_long), ("num_glyphs", ctypes.c_long),
                ("family_name", ctypes.c_char_p), ("style_name", ctypes.c_char_p),
                ("num_fixed_sizes", ctypes.c_int), ("available_sizes", ctypes.c_void_p),
                ("num_charmaps", ctypes.c_int), ("charmaps", ctypes.c_void_p), ("generic", FtGeneric),
                ("bbox", ctypes.c_long * 4), ("units_per_EM", ctypes.c_ushort),
                ("ascender", ctypes.c_short), ("descender", ctypes.c_short), ("height", ctypes.c_short),
                ("max_advance_width", ctypes.c_short), ("max_advance_height", ctypes.c_short),
                ("underline_position", ctypes.c_short), ("underline_thickness", ctypes.c_short),
                ("glyph", ctypes.POINTER(FtGlyphSlot))]


FT_LOAD_NO_SCALE = 1 << 0
FT_CURVE_TAG_ON = 1
FT_CURVE_TAG_CUBIC = 2


class EmboldeningFace:
    """시스템 libfreetype으로 글리프 윤곽선을 읽고 Skia와 같은 세기로 굵게 만든다"""

    def __init__(self, path: str, face_index: int):
        self.freetype = ctypes.CDLL("libfreetype.so.6")
        self.library = ctypes.c_void_p()
        self.check(self.freetype.FT_Init_FreeType(ctypes.byref(self.library)), "FT_Init_FreeType")
        self.face = ctypes.POINTER(FtFace)()
        self.freetype.FT_New_Face.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_long,
                                              ctypes.POINTER(ctypes.POINTER(FtFace))]
        self.check(self.freetype.FT_New_Face(self.library, path.encode(), face_index, ctypes.byref(self.face)),
                   "FT_New_Face")
        self.freetype.FT_Get_Char_Index.argtypes = [ctypes.POINTER(FtFace), ctypes.c_ulong]
        self.freetype.FT_Get_Char_Index.restype = ctypes.c_uint
        self.freetype.FT_Load_Glyph.argtypes = [ctypes.POINTER(FtFace), ctypes.c_uint, ctypes.c_int32]
        self.freetype.FT_Outline_Transform.argtypes = [ctypes.POINTER(FtOutline), ctypes.POINTER(FtMatrix)]
        self.freetype.FT_Outline_Embolden.argtypes = [ctypes.POINTER(FtOutline), ctypes.c_long]
        self.units_per_em = self.face.contents.units_per_EM
        self.strength = round(self.units_per_em * OUTLINE_PRECISION / EMBOLDEN_DIVISOR)

    @staticmethod
    def check(error: int, call: str) -> None:
        if error:
            raise RuntimeError(f"{call} failed with FreeType error {error}")

    def glyph_index(self, codepoint: int) -> int:
        return self.freetype.FT_Get_Char_Index(self.face, codepoint)

    def emboldened_glyph(self, glyph_index: int) -> tuple[int, list[list[tuple[int, int, int]]]]:
        """(원본 advance, 윤곽선 목록). 윤곽선은 (x, y, tag) 점 목록이고 좌표는 글꼴 단위 정수다"""
        self.check(self.freetype.FT_Load_Glyph(self.face, glyph_index, FT_LOAD_NO_SCALE), "FT_Load_Glyph")
        slot = self.face.contents.glyph.contents
        advance = slot.metrics.horiAdvance
        outline = slot.outline
        if outline.n_points == 0:
            return advance, []
        scale_up = FtMatrix(OUTLINE_PRECISION << 16, 0, 0, OUTLINE_PRECISION << 16)
        self.freetype.FT_Outline_Transform(ctypes.byref(outline), ctypes.byref(scale_up))
        self.check(self.freetype.FT_Outline_Embolden(ctypes.byref(outline), self.strength), "FT_Outline_Embolden")
        contours, start = [], 0
        for contour_index in range(outline.n_contours):
            end = outline.contours[contour_index]
            contours.append([(round(outline.points[point].x / OUTLINE_PRECISION),
                              round(outline.points[point].y / OUTLINE_PRECISION),
                              outline.tags[point] & 3) for point in range(start, end + 1)])
            start = end + 1
        return advance, contours


# ───────────── CFF 쓰기 ─────────────

def encode_operand(value: int) -> bytes:
    """Type 2 charstring 정수 피연산자"""
    if -107 <= value <= 107:
        return bytes([value + 139])
    if 108 <= value <= 1131:
        value -= 108
        return bytes([(value >> 8) + 247, value & 0xFF])
    if -1131 <= value <= -108:
        value = -value - 108
        return bytes([(value >> 8) + 251, value & 0xFF])
    return b"\x1c" + struct.pack(">h", value)


def encode_dict_operand(value: int) -> bytes:
    """DICT 정수. 오프셋이 값에 따라 길이를 바꾸지 않도록 항상 5바이트 형식을 쓴다"""
    return b"\x1d" + struct.pack(">i", value)


def encode_index(items: list[bytes]) -> bytes:
    if not items:
        return b"\0\0"
    offsets = [1]
    for item in items:
        offsets.append(offsets[-1] + len(item))
    offset_size = 1 if offsets[-1] < 1 << 8 else 2 if offsets[-1] < 1 << 16 else 3 if offsets[-1] < 1 << 24 else 4
    packed_offsets = b"".join(offset.to_bytes(offset_size, "big") for offset in offsets)
    return struct.pack(">HB", len(items), offset_size) + packed_offsets + b"".join(items)


def contour_segments(contour):
    """첫 점을 on-curve로 맞춘 뒤 (끝점, [제어점...]) 구간을 돌려준다. 마지막 구간은 시작점으로 돌아온다"""
    first_on = next(index for index, point in enumerate(contour) if point[2] & FT_CURVE_TAG_ON)
    rotated = contour[first_on:] + contour[:first_on]
    start = rotated[0]
    segments, controls = [], []
    for point in rotated[1:] + [start]:
        if point[2] & FT_CURVE_TAG_ON:
            segments.append(((point[0], point[1]), controls))
            controls = []
        else:
            if not point[2] & FT_CURVE_TAG_CUBIC:
                raise ValueError("quadratic outline in a CFF face")
            controls.append((point[0], point[1]))
    return (start[0], start[1]), segments


def encode_charstring(contours) -> bytes:
    charstring = bytearray()
    current = (0, 0)

    def emit(points, operator):
        nonlocal current
        for x, y in points:
            charstring.extend(encode_operand(x - current[0]) + encode_operand(y - current[1]))
            current = (x, y)
        charstring.append(operator)

    for contour in contours:
        start, segments = contour_segments(contour)
        emit([start], 21)  # rmoveto
        for index, (end, controls) in enumerate(segments):
            is_closing_line = index == len(segments) - 1 and not controls
            if is_closing_line:
                continue  # CFF는 윤곽선을 시작점까지 저절로 닫는다
            if controls:
                emit(controls + [end], 8)  # rrcurveto
            else:
                emit([end], 5)  # rlineto
    charstring.append(14)  # endchar
    return bytes(charstring)


def build_cff(postscript_name: str, charstrings: list[bytes], glyph_names: list[str], font_bbox) -> bytes:
    header = bytes([1, 0, 4, 4])
    name_index = encode_index([postscript_name.encode("ascii")])
    string_index = encode_index([name.encode("ascii") for name in glyph_names[1:]])
    global_subrs = encode_index([])
    # charset 형식 2: SID 391(첫 사용자 문자열)부터 연속 범위 하나
    charset = bytes([2]) + struct.pack(">HH", 391, len(glyph_names) - 2) if len(glyph_names) > 1 else bytes([0])
    charstrings_index = encode_index(charstrings)
    private_dict = encode_operand(0) + bytes([20])  # defaultWidthX 0(advance는 hmtx가 정한다)

    def top_dict(charset_offset, charstrings_offset, private_offset):
        return (b"".join(encode_dict_operand(value) for value in font_bbox) + bytes([5])
                + encode_dict_operand(charset_offset) + bytes([15])
                + encode_dict_operand(charstrings_offset) + bytes([17])
                + encode_dict_operand(len(private_dict)) + encode_dict_operand(private_offset) + bytes([18]))

    fixed_size = len(header) + len(name_index) + len(encode_index([top_dict(0, 0, 0)])) + len(string_index) + len(
        global_subrs)
    charset_offset = fixed_size
    charstrings_offset = charset_offset + len(charset)
    private_offset = charstrings_offset + len(charstrings_index)
    top_dict_index = encode_index([top_dict(charset_offset, charstrings_offset, private_offset)])
    return (header + name_index + top_dict_index + string_index + global_subrs + charset + charstrings_index
            + private_dict)


# ───────────── 합성 bold 얼굴 ─────────────

def build_cmap(code_to_glyph: dict[int, int]) -> bytes:
    """(3,1) 형식 4 하위 표. 코드와 글리프 번호가 함께 1씩 느는 구간을 idDelta 한 구간으로 묶는다"""
    segments = []
    for code in sorted(code_to_glyph):
        glyph = code_to_glyph[code]
        if segments and segments[-1][1] == code - 1 and segments[-1][2] + (code - segments[-1][0]) == glyph:
            segments[-1][1] = code
        else:
            segments.append([code, code, glyph])
    segments.append([0xFFFF, 0xFFFF, 1])
    segment_count = len(segments)
    entry_selector = int(math.log2(segment_count))
    search_range = 2 * (1 << entry_selector)
    ends = b"".join(struct.pack(">H", end) for _, end, _ in segments)
    starts = b"".join(struct.pack(">H", start) for start, _, _ in segments)
    deltas = b"".join(struct.pack(">H", (glyph - start) & 0xFFFF) for start, _, glyph in segments)
    range_offsets = b"\0\0" * segment_count
    body = (struct.pack(">HHHH", segment_count * 2, search_range, entry_selector, segment_count * 2 - search_range)
            + ends + b"\0\0" + starts + deltas + range_offsets)
    subtable = struct.pack(">HHH", 4, 6 + len(body), 0) + body
    return struct.pack(">HHHHI", 0, 1, 3, 1, 12) + subtable


def message_characters() -> set[int]:
    characters: set[int] = set()

    def collect(value):
        if isinstance(value, dict):
            for item in value.values():
                collect(item)
        elif isinstance(value, list):
            for item in value:
                collect(item)
        elif isinstance(value, str):
            characters.update(ord(character) for character in value)

    for message_path in sorted(MESSAGES_DIR.glob("*.json")):
        collect(json.loads(message_path.read_text(encoding="utf-8")))
    return characters


def sfnt_covers(tables: dict[str, bytes], codepoint: int) -> bool:
    """(3,1)/(3,10)/(0,*) 형식 4·12 cmap에 글자가 있는지"""
    cmap = tables["cmap"]
    _, subtable_count = struct.unpack(">HH", cmap[:4])
    for index in range(subtable_count):
        platform_id, encoding_id, offset = struct.unpack(">HHI", cmap[4 + 8 * index:12 + 8 * index])
        if (platform_id, encoding_id) not in ((3, 1), (3, 10), (0, 3), (0, 4)):
            continue
        (subtable_format,) = struct.unpack(">H", cmap[offset:offset + 2])
        if subtable_format == 4 and codepoint <= 0xFFFF:
            (segment_count_x2,) = struct.unpack(">H", cmap[offset + 6:offset + 8])
            count = segment_count_x2 // 2
            ends = struct.unpack(f">{count}H", cmap[offset + 14:offset + 14 + segment_count_x2])
            starts_offset = offset + 16 + segment_count_x2
            starts = struct.unpack(f">{count}H", cmap[starts_offset:starts_offset + segment_count_x2])
            if any(start <= codepoint <= end for start, end in zip(starts, ends) if start != 0xFFFF):
                return True
        elif subtable_format == 12:
            (group_count,) = struct.unpack(">I", cmap[offset + 12:offset + 16])
            for group in range(group_count):
                start, end, _ = struct.unpack(">III", cmap[offset + 16 + 12 * group:offset + 28 + 12 * group])
                if start <= codepoint <= end:
                    return True
    return False


def synthetic_bold_codepoints(face: EmboldeningFace, latin_bold_tables: dict[str, bytes]) -> list[int]:
    wanted = {codepoint for first, last in SYNTHETIC_BOLD_RANGES for codepoint in range(first, last + 1)}
    wanted |= {codepoint for codepoint in message_characters()
               if codepoint > 0x7E and not sfnt_covers(latin_bold_tables, codepoint)}
    return sorted(codepoint for codepoint in wanted if codepoint <= 0xFFFF and face.glyph_index(codepoint))


def build_synthetic_bold(cjk_data: bytes, cjk_face_index: int, cjk_face_offset: int, latin_bold_data: bytes) -> bytes:
    face = EmboldeningFace(CJK_SOURCE_PATH, cjk_face_index)
    _, source_tables = read_tables(cjk_data, cjk_face_offset)
    _, latin_bold_tables = read_tables(latin_bold_data, 0)
    codepoints = synthetic_bold_codepoints(face, latin_bold_tables)

    # 새 글리프 번호는 .notdef 다음부터 글자 순서대로 매기고, 여러 글자가 같은 원본 글리프를 쓰면 한 번만 넣는다
    glyph_of_source = {0: 0}
    code_to_glyph = {}
    for codepoint in codepoints:
        source_glyph = face.glyph_index(codepoint)
        code_to_glyph[codepoint] = glyph_of_source.setdefault(source_glyph, len(glyph_of_source))
    source_glyphs = list(glyph_of_source)

    charstrings, metrics, glyph_names = [], [], [".notdef"]
    bbox = [0, 0, 0, 0]
    for new_glyph, source_glyph in enumerate(source_glyphs):
        advance, contours = face.emboldened_glyph(source_glyph)
        charstrings.append(encode_charstring(contours))
        points = [(x, y) for contour in contours for x, y, _ in contour]
        x_min = min((x for x, _ in points), default=0)
        metrics.append((advance, x_min, max((x for x, _ in points), default=0)))
        if points:
            bbox = [min(bbox[0], x_min), min(bbox[1], min(y for _, y in points)),
                    max(bbox[2], max(x for x, _ in points)), max(bbox[3], max(y for _, y in points))]
        if new_glyph:
            glyph_names.append(f"glyph{source_glyph}")

    hmtx = b"".join(struct.pack(">Hh", advance, left) for advance, left, _ in metrics)
    hhea = bytearray(source_tables["hhea"])
    struct.pack_into(">HhhH", hhea, 10, max(advance for advance, _, _ in metrics),
                     min(left for _, left, _ in metrics),
                     min(advance - right for advance, _, right in metrics), max(right for _, _, right in metrics))
    struct.pack_into(">H", hhea, 34, len(metrics))
    head = bytearray(source_tables["head"])
    struct.pack_into(">hhhhH", head, 36, *bbox, struct.unpack(">H", head[44:46])[0] | 0x1)
    os2 = bytearray(source_tables["OS/2"])
    struct.pack_into(">H", os2, 4, 700)
    fs_selection = (struct.unpack(">H", os2[62:64])[0] | 0x20) & ~0x40
    struct.pack_into(">HHH", os2, 62, fs_selection, min(codepoints), max(codepoints))
    post = struct.pack(">I", 0x00030000) + source_tables["post"][4:32]
    name_records = [record for record in read_name_records(source_tables["name"])
                    if record[3] in (0, 5, 7, 8, 9, 11, 12, 13, 14) and is_english_record(record[0], record[2])]
    for name_id, text in ((1, SYNTHETIC_BOLD_FAMILY), (2, "Bold"), (3, SYNTHETIC_BOLD_POSTSCRIPT),
                          (4, SYNTHETIC_BOLD_FAMILY), (6, SYNTHETIC_BOLD_POSTSCRIPT),
                          (10, "Noto Sans CJK KR Regular emboldened with FT_Outline_Embolden(upem/24) "
                               "to match Chromium synthetic bold; built by parity/tools/fonts/build_app_fonts.py")):
        name_records.append((*WINDOWS_ENGLISH, name_id, text))
    tables = {
        "CFF ": build_cff(SYNTHETIC_BOLD_POSTSCRIPT, charstrings, glyph_names, bbox),
        "OS/2": bytes(os2),
        "cmap": build_cmap(code_to_glyph),
        "head": bytes(head),
        "hhea": bytes(hhea),
        "hmtx": hmtx,
        "maxp": struct.pack(">IH", 0x00005000, len(metrics)),
        "name": build_name_table(name_records),
        "post": post,
    }
    print(f"synthetic bold: {len(codepoints)} characters, {len(metrics)} glyphs, strength {face.strength}/"
          f"{OUTLINE_PRECISION} units (upem {face.units_per_em})")
    return write_sfnt(0x4F54544F, tables)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--out", default=str(DEFAULT_OUTPUT_DIR))
    args = parser.parse_args()
    output_dir = Path(args.out)
    output_dir.mkdir(parents=True, exist_ok=True)

    sources = {path: read_verified(path, sha256) for path, sha256, _ in SOURCE_FONTS}
    for path, _, output_name in SOURCE_FONTS:
        _, face_offset = find_face(sources[path], CJK_FACE_FAMILY) if path == CJK_SOURCE_PATH else (0, 0)
        (output_dir / output_name).write_bytes(renamed_font(sources[path], face_offset))
        print(f"renamed: {output_name}")

    cjk_face_index, cjk_face_offset = find_face(sources[CJK_SOURCE_PATH], CJK_FACE_FAMILY)
    synthetic_bold = build_synthetic_bold(sources[CJK_SOURCE_PATH], cjk_face_index, cjk_face_offset,
                                          sources[LATIN_BOLD_SOURCE_PATH])
    (output_dir / SYNTHETIC_BOLD_OUTPUT).write_bytes(synthetic_bold)
    for _, _, output_name in SOURCE_FONTS:
        print(f"{output_name} sha256 {hashlib.sha256((output_dir / output_name).read_bytes()).hexdigest()}")
    print(f"{SYNTHETIC_BOLD_OUTPUT} sha256 {hashlib.sha256(synthetic_bold).hexdigest()}")


if __name__ == "__main__":
    main()
