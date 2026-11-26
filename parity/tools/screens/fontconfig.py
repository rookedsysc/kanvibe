#!/usr/bin/env python3
"""앱 프로세스에만 넘기는 fontconfig 설정(FONTCONFIG_FILE)을 만든다.

parity/screens/fonts.json에 적힌 글꼴 파일만 보이게 해서, 시스템에 글꼴이 새로 깔려도 기준 이미지가 바뀌지 않게 한다.
렌더링 옵션(안티에일리어싱, 힌팅, 서브픽셀)도 여기서 고정한다. GPUI 앱은 같은 파일과 같은 옵션을 써야 한다.
"""
from __future__ import annotations

import json
import shutil
from pathlib import Path
from xml.sax.saxutils import escape

SCREENS_DIR = Path(__file__).resolve().parents[2] / "screens"
FONTS_MANIFEST = SCREENS_DIR / "fonts.json"


def render_alias(family: str, preferred: list[str]) -> str:
    prefer = "".join(f"<family>{escape(name)}</family>" for name in preferred)
    return f"  <alias binding=\"strong\"><family>{escape(family)}</family><prefer>{prefer}</prefer></alias>\n"


def render_fallback_order(families: list[str]) -> str:
    """글자별 대체 글꼴을 찾을 때(family 없이 charset·lang만 있는 질의) 고를 순서를 고정한다."""
    values = "".join(f"<string>{escape(name)}</string>" for name in families)
    return f"  <match target=\"pattern\"><edit name=\"family\" mode=\"append_last\">{values}</edit></match>\n"


def write_pinned_fontconfig(output_dir: Path, use_system_fonts: bool = False) -> Path:
    if output_dir.exists():
        shutil.rmtree(output_dir)
    fonts_dir = output_dir / "fonts"
    fonts_dir.mkdir(parents=True)
    manifest = json.loads(FONTS_MANIFEST.read_text(encoding="utf-8"))

    if use_system_fonts:
        font_dirs = "  <include ignore_missing=\"yes\">/etc/fonts/fonts.conf</include>\n"
    else:
        for font in manifest["files"]:
            source = Path(font["path"])
            if not source.exists():
                raise FileNotFoundError(f"pinned font is missing: {source}")
            (fonts_dir / source.name).symlink_to(source)
        font_dirs = f"  <dir>{escape(str(fonts_dir))}</dir>\n"

    rendering = manifest["rendering"]
    aliases = "".join(render_alias(family, preferred) for family, preferred in manifest["aliases"].items())
    config = (
        "<?xml version=\"1.0\"?>\n<!DOCTYPE fontconfig SYSTEM \"urn:fontconfig:fonts.dtd\">\n<fontconfig>\n"
        f"{font_dirs}"
        f"  <cachedir>{escape(str(output_dir / 'cache'))}</cachedir>\n"
        f"{aliases}"
        f"{render_fallback_order(manifest['fallback_order'])}"
        "  <match target=\"font\">\n"
        f"    <edit name=\"antialias\" mode=\"assign\"><bool>{str(rendering['antialias']).lower()}</bool></edit>\n"
        f"    <edit name=\"hinting\" mode=\"assign\"><bool>{str(rendering['hinting']).lower()}</bool></edit>\n"
        f"    <edit name=\"hintstyle\" mode=\"assign\"><const>{rendering['hintstyle']}</const></edit>\n"
        f"    <edit name=\"rgba\" mode=\"assign\"><const>{rendering['rgba']}</const></edit>\n"
        f"    <edit name=\"lcdfilter\" mode=\"assign\"><const>{rendering['lcdfilter']}</const></edit>\n"
        f"    <edit name=\"autohint\" mode=\"assign\"><bool>{str(rendering['autohint']).lower()}</bool></edit>\n"
        f"    <edit name=\"embeddedbitmap\" mode=\"assign\"><bool>false</bool></edit>\n"
        "  </match>\n"
        "</fontconfig>\n"
    )
    config_path = output_dir / "fonts.conf"
    config_path.write_text(config, encoding="utf-8")
    return config_path
