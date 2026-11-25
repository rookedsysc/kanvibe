#!/usr/bin/env python3
"""Mermaid 기준 이미지를 TS 앱의 실제 모바일 다이어그램 경로로 찍는다.

parity/diagrams/baseline/<id>.mmd 마다 light·dark 두 테마로 빌드된 renderDiagramInDesktop()을 불러
JPEG를 받고, 픽셀 비교용 PNG(<id>--<theme>.png)와 좌표 확인용 SVG(<id>--<theme>.svg)를 남긴다.
데스크톱 화면(AiSessionMessageContent)은 워커와 다른 설정으로 `mermaid.initialize`를 부른다: `htmlLabels: false`(SVG 글자
라벨)를 주고 `secure` 목록을 바꾸지 않아 정의의 `themeVariables`·`theme`·`themeCSS`가 먹는다. 그래서 빌드된 워커 번들의
initialize 인자만 데스크톱 설정으로 바꾼 사본을 임시 앱에 두고 `<id>--desktop--<theme>`를 그 사본으로 찍는다.
크기 맞춤과 프레임 캡처는 모바일 경로와 같지만, 데스크톱 화면에는 JPEG 단계가 없어 같은 프레임을 손실 없는 PNG로 받는다.
Electron은 xvfb-run 아래에서 격리된 KANVIBE_APP_DATA_DIR·HOME·FONTCONFIG_FILE을 받은 자식 프로세스로만 돈다.
필요한 것: xvfb-run, 저장소 node_modules(Electron), `pnpm build` 산출물(build/), Pillow.
"""
from __future__ import annotations

import argparse
import json
import os
import re
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
WORKER_DOCUMENT = "diagram-worker.html"
# 워커 번들의 mermaid.initialize 호출. 축소된 변수 이름(Mermaid 객체, 테마 인자)을 함께 잡는다
WORKER_INITIALIZE = re.compile(
    r'(\w+)\.initialize\(\{startOnLoad:!1,securityLevel:"strict",theme:(\w+)==="dark"\?"dark":"default",[^{}]*?secure:\[[^\]]*\]\}\)'
)
# AiSessionMessageContent의 mermaidApi.initialize 인자와 같은 설정(라이트 앱 테마는 default, 나머지는 dark)
DESKTOP_INITIALIZE = r'\1.initialize({securityLevel:"strict",startOnLoad:!1,htmlLabels:!1,theme:\2==="dark"?"dark":"default"})'


def prepare_app_dir(work_dir: Path) -> Path:
    """빌드 산출물과 electron 디렉터리를 가리키는 임시 앱을 만든다. app.getAppPath()가 이 디렉터리가 된다."""
    app_dir = work_dir / "app"
    if app_dir.exists():
        shutil.rmtree(app_dir)
    app_dir.mkdir(parents=True)
    prepare_build_dir(app_dir / "build")
    (app_dir / "electron").symlink_to(REPO_ROOT / "electron")
    for name in ("driver.cjs", "svg-preload.cjs"):
        shutil.copy(DRIVER_DIR / name, app_dir / name)
    (app_dir / "package.json").write_text(json.dumps({"name": "kanvibe-diagram-parity", "main": "driver.cjs"}), encoding="utf-8")
    return app_dir


def link_children(source: Path, target: Path, skip: set[str]) -> None:
    target.mkdir(parents=True)
    for child in source.iterdir():
        if child.name not in skip:
            (target / child.name).symlink_to(child)


def prepare_build_dir(build_dir: Path) -> None:
    """저장소 build/를 링크로 비추되, 워커 문서와 그 번들은 설정별 사본을 둔다.

    driver.cjs가 그리기 전마다 `renderer/diagram-worker.<configuration>.html`을 `diagram-worker.html`로 옮겨 놓는다.
    """
    source_renderer = REPO_ROOT / "build" / "renderer"
    link_children(REPO_ROOT / "build", build_dir, {"renderer"})
    renderer_dir = build_dir / "renderer"
    link_children(source_renderer, renderer_dir, {"assets", WORKER_DOCUMENT})
    link_children(source_renderer / "assets", renderer_dir / "assets", set())
    document = (source_renderer / WORKER_DOCUMENT).read_text(encoding="utf-8")
    script_match = re.search(r'src="\./assets/(diagramWorker-[^"]+\.js)"', document)
    if script_match is None:
        raise SystemExit("diagram-worker.html에서 워커 번들을 찾지 못했습니다")
    mobile_script = script_match.group(1)
    desktop_script = mobile_script.replace("diagramWorker-", "diagramWorkerDesktop-")
    bundle = (source_renderer / "assets" / mobile_script).read_text(encoding="utf-8")
    desktop_bundle, replaced = WORKER_INITIALIZE.subn(DESKTOP_INITIALIZE, bundle)
    if replaced != 1:
        raise SystemExit(f"워커 번들의 mermaid.initialize 호출을 하나로 찾지 못했습니다({replaced}개)")
    (renderer_dir / "assets" / desktop_script).write_text(desktop_bundle, encoding="utf-8")
    (renderer_dir / "diagram-worker.mobile.html").write_text(document, encoding="utf-8")
    desktop_document = document.replace(f"./assets/{mobile_script}", f"./assets/{desktop_script}")
    (renderer_dir / "diagram-worker.desktop.html").write_text(desktop_document, encoding="utf-8")


def build_entries(only: set[str] | None, width: int, configurations: tuple[str, ...]) -> list[dict]:
    entries = []
    for source in sorted(BASELINE_DIR.glob("*.mmd")):
        if only and source.stem not in only:
            continue
        definition = source.read_text(encoding="utf-8").strip()
        for configuration, entry_id in (("mobile", source.stem), ("desktop", f"{source.stem}--desktop")):
            if configuration not in configurations:
                continue
            entries.extend(
                {"id": entry_id, "configuration": configuration, "definition": definition, "theme": theme, "width": width}
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
    """모바일 JPEG는 모바일이 받는 원본이고 데스크톱은 손실 없는 PNG다. 비교 도구가 PNG만 읽으므로 그대로 PNG로 옮긴다."""
    results = json.loads((out_dir / "results.json").read_text(encoding="utf-8"))
    for result in results:
        stem = f"{result['id']}--{result['theme']}"
        captured = next((out_dir / f"{stem}.{extension}" for extension in ("png", "jpg") if (out_dir / f"{stem}.{extension}").exists()), None)
        if captured is not None:
            with Image.open(captured) as image:
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
    parser.add_argument("--configuration", choices=("mobile", "desktop", "both"), default="both",
                        help="찍을 경로(모바일 JPEG, 데스크톱 PNG, 둘 다)")
    args = parser.parse_args()

    work_dir = Path(args.work_dir)
    out_dir = work_dir / "out"
    shutil.rmtree(out_dir, ignore_errors=True)
    out_dir.mkdir(parents=True)
    only = set(args.only.split(",")) if args.only else None
    configurations = ("mobile", "desktop") if args.configuration == "both" else (args.configuration,)
    entries = build_entries(only, args.width, configurations)
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
