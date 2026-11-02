# /// script
# requires-python = ">=3.10"
# dependencies = ["Pillow==13.0.0"]
# ///
"""제공된 브랜드 원본에서 앱별 로고와 런처 아이콘을 생성한다."""

import json
from pathlib import Path

from PIL import Image, ImageDraw, ImageOps

ROOT = Path(__file__).resolve().parents[1]
BRANDING = ROOT / "resources/branding"
ANDROID_RES = ROOT / "mobile/android/app/src/main/res"
IOS_ICONS = ROOT / "mobile/ios/Runner/Assets.xcassets/AppIcon.appiconset"
DENSITIES = {"mdpi": 1, "hdpi": 1.5, "xhdpi": 2, "xxhdpi": 3, "xxxhdpi": 4}


def save_png(image: Image.Image, destination: Path) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    image.save(destination, optimize=True)


def centered_mark(emblem: Image.Image, size: int, mark_size: int) -> Image.Image:
    canvas = Image.new("RGBA", (size, size))
    mark = ImageOps.contain(emblem, (mark_size, mark_size), Image.Resampling.LANCZOS)
    canvas.alpha_composite(mark, ((size - mark.width) // 2, (size - mark.height) // 2))
    return canvas


def opaque_icon(emblem: Image.Image, size: int) -> Image.Image:
    foreground = centered_mark(emblem, size, round(size * 2 / 3))
    background = Image.new("RGBA", foreground.size, "white")
    return Image.alpha_composite(background, foreground).convert("RGB")


def main() -> None:
    logo = Image.open(BRANDING / "kanvibe-logo.png").convert("RGBA")
    emblem = Image.open(BRANDING / "kanvibe-emblem.png").convert("RGBA")
    logo = logo.crop(logo.getbbox())
    emblem = emblem.crop(emblem.getbbox())
    logo = ImageOps.contain(logo, (1024, 1024), Image.Resampling.LANCZOS)
    for destination in ("public/branding/kanvibe-logo.png", "mobile/assets/branding/kanvibe-logo.png"):
        save_png(logo, ROOT / destination)
    save_png(opaque_icon(emblem, 64), ROOT / "public/favicon.png")

    desktop_icon = Image.new("RGBA", (1024, 1024))
    ImageDraw.Draw(desktop_icon).rounded_rectangle((100, 100, 923, 923), radius=184, fill="white")
    desktop_icon.alpha_composite(centered_mark(emblem, 1024, 560))
    save_png(desktop_icon, ROOT / "public/branding/app-icon.png")
    desktop_icon.save(ROOT / "resources/icon.icns", format="ICNS")

    catalog = json.loads((IOS_ICONS / "Contents.json").read_text())
    for entry in catalog["images"]:
        size = round(float(entry["size"].split("x")[0]) * float(entry["scale"].removesuffix("x")))
        save_png(opaque_icon(emblem, size), IOS_ICONS / entry["filename"])

    for density, scale in DENSITIES.items():
        save_png(opaque_icon(emblem, round(48 * scale)), ANDROID_RES / f"mipmap-{density}/ic_launcher.png")
        foreground = centered_mark(emblem, round(108 * scale), round(48 * scale))
        save_png(foreground, ANDROID_RES / f"drawable-{density}/ic_launcher_foreground.png")


if __name__ == "__main__":
    main()
