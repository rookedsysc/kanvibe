#!/usr/bin/env python3
"""Mermaid 기준 이미지를 TS 앱의 실제 모바일 다이어그램 경로로 찍는다.

parity/diagrams/baseline/<id>.mmd 마다 light·dark 두 테마로 빌드된 renderDiagramInDesktop()을 불러
JPEG를 받고, 픽셀 비교용 PNG(<id>--<theme>.png)와 좌표 확인용 SVG(<id>--<theme>.svg)를 남긴다.
데스크톱 화면(AiSessionMessageContent)은 `htmlLabels: false`(SVG 글자 라벨)로 그리므로, 같은 워커에 같은 설정을
init 지시문으로 넘겨 `<id>--desktop--<theme>`도 함께 남긴다. 설정 키가 같고 `secure` 목록에 없어 initialize와 같게 먹는다.
Electron은 xvfb-run 아래에서 격리된 KANVIBE_APP_DATA_DIR·HOME·FONTCONFIG_FILE을 받은 자식 프로세스로만 돈다.
필요한 것: xvfb-run, 저장소 node_modules(Electron), `pnpm build` 산출물(build/), Pillow.
"""
from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

from PIL import Image

REPO_ROOT = Path(__file__).resolve().parents[3]
BASELINE_DIR = REPO_ROOT / "parity" / "diagrams" / "baseline"
DRIVER_DIR = Path(__file__).resolve().parent / "ts-driver"
sys.path.insert(0, str(REPO_ROOT / "parity" / "tools" / "screens"))
from fontconfig import write_pinned_fontconfig  # noqa: E402

DEFAULT_WIDTH = 800
THEMES = ("light", "dark")
# 데스크톱 렌더러의 mermaid.initialize({ htmlLabels: false })와 같은 설정
DESKTOP_DIRECTIVE = '%%{init: {"htmlLabels": false}}%%'


def prepare_app_dir(work_dir: Path) -> Path:
    """빌드 산출물과 electron 디렉터리를 가리키는 임시 앱을 만든다. app.getAppPath()가 이 디렉터리가 된다."""
    app_dir = work_dir / "app"
    if app_dir.exists():
        shutil.rmtree(app_dir)
    app_dir.mkdir(parents=True)
    (app_dir / "build").symlink_to(REPO_ROOT / "build")
    (app_dir / "electron").symlink_to(REPO_ROOT / "electron")
    for name in ("driver.cjs", "svg-preload.cjs"):
        shutil.copy(DRIVER_DIR / name, app_dir / name)
    (app_dir / "package.json").write_text(json.dumps({"name": "kanvibe-diagram-parity", "main": "driver.cjs"}), encoding="utf-8")
    return app_dir


def with_desktop_directive(definition: str) -> str:
    """YAML 앞머리는 정의 맨 앞에 있어야 하므로 지시문을 그 뒤에 넣는다."""
    lines = definition.split("\n")
    if lines[0].strip() == "---":
        closing = next((index for index in range(1, len(lines)) if lines[index].strip() == "---"), None)
        if closing is not None:
            return "\n".join([*lines[: closing + 1], DESKTOP_DIRECTIVE, *lines[closing + 1 :]])
    return f"{DESKTOP_DIRECTIVE}\n{definition}"


def build_entries(only: set[str] | None, width: int) -> list[dict]:
    entries = []
    for source in sorted(BASELINE_DIR.glob("*.mmd")):
        if only and source.stem not in only:
            continue
        definition = source.read_text(encoding="utf-8").strip()
        entries.extend({"id": source.stem, "definition": definition, "theme": theme, "width": width} for theme in THEMES)
        desktop_definition = with_desktop_directive(definition)
        entries.extend(
            {"id": f"{source.stem}--desktop", "definition": desktop_definition, "theme": theme, "width": width}
            for theme in THEMES
        )
    return entries


def isolated_env(work_dir: Path, fontconfig_path: Path) -> dict:
    home = work_dir / "home"
    for directory in (home, work_dir / "app-data", work_dir / "xdg-runtime"):
        directory.mkdir(parents=True, exist_ok=True)
    (work_dir / "xdg-runtime").chmod(0o700)
    return {
        "PATH": os.environ["PATH"],
        "HOME": str(home),
        "XDG_CONFIG_HOME": str(home / ".config"),
        "XDG_RUNTIME_DIR": str(work_dir / "xdg-runtime"),
        "KANVIBE_APP_DATA_DIR": str(work_dir / "app-data"),
        "FONTCONFIG_FILE": str(fontconfig_path),
    }


def convert_outputs(out_dir: Path) -> list[dict]:
    """JPEG는 모바일이 받는 원본이고, 비교 도구가 PNG만 읽으므로 손실 없이 PNG로 옮긴다."""
    results = json.loads((out_dir / "results.json").read_text(encoding="utf-8"))
    for result in results:
        stem = f"{result['id']}--{result['theme']}"
        jpeg = out_dir / f"{stem}.jpg"
        if jpeg.exists():
            with Image.open(jpeg) as image:
                result["height"] = image.height
                image.convert("RGB").save(BASELINE_DIR / f"{stem}.png")
        svg = out_dir / f"{stem}.svg"
        if svg.exists():
            shutil.copy(svg, BASELINE_DIR / f"{stem}.svg")
    return results


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--only", help="쉼표로 구분한 다이어그램 id")
    parser.add_argument("--width", type=int, default=DEFAULT_WIDTH)
    parser.add_argument("--work-dir", default="/tmp/kanvibe-diagram-parity")
    args = parser.parse_args()

    work_dir = Path(args.work_dir)
    out_dir = work_dir / "out"
    shutil.rmtree(out_dir, ignore_errors=True)
    out_dir.mkdir(parents=True)
    only = set(args.only.split(",")) if args.only else None
    entries = build_entries(only, args.width)
    job_path = work_dir / "job.json"
    job_path.write_text(json.dumps({"repoRoot": str(REPO_ROOT), "outDir": str(out_dir), "entries": entries}), encoding="utf-8")

    app_dir = prepare_app_dir(work_dir)
    fontconfig_path = write_pinned_fontconfig(work_dir / "fontconfig")
    command = [
        "xvfb-run", "-a", "-s", "-screen 0 1600x1000x24 -dpi 96",
        str(REPO_ROOT / "node_modules" / ".bin" / "electron"), str(app_dir),
        "--no-sandbox", "--force-device-scale-factor=1", "--force-color-profile=srgb", "--disable-lcd-text",
        str(job_path),
    ]
    completed = subprocess.run(command, env=isolated_env(work_dir, fontconfig_path), timeout=600)
    if completed.returncode != 0:
        print(f"electron driver failed: {completed.returncode}", file=sys.stderr)
        return 1
    results = convert_outputs(out_dir)
    manifest_path = BASELINE_DIR / "manifest.json"
    manifest = json.loads(manifest_path.read_text(encoding="utf-8")) if manifest_path.exists() else {}
    for result in results:
        manifest[f"{result['id']}--{result['theme']}"] = result
    manifest_path.write_text(json.dumps(dict(sorted(manifest.items())), ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    failed = [key for key, value in manifest.items() if not value.get("success")]
    print(f"captured {len(results)} images, failed: {failed or 'none'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
