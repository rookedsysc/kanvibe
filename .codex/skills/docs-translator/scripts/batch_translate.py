#!/usr/bin/env python3
"""
Batch translation utility for documentation pages.
Optimizes translation by caching common phrases and grouping similar content.
"""

import json
import os
import re
import sys
from pathlib import Path
from typing import Dict, List

# 번역 캐시 (일관성 유지를 위한 공통 문구)
TRANSLATION_CACHE = {
    "Getting Started": "시작하기",
    "Quick Start": "빠른 시작",
    "Installation": "설치",
    "Configuration": "구성",
    "API Reference": "API 레퍼런스",
    "Examples": "예제",
    "Advanced": "고급",
    "Guides": "가이드",
    "Tutorial": "튜토리얼",
    "Documentation": "문서",
}


def extract_code_blocks(content: str) -> tuple[str, List[str]]:
    """
    코드 블록을 추출하고 플레이스홀더로 대체

    Returns:
        tuple: (코드 블록이 제거된 텍스트, 코드 블록 리스트)
    """
    code_blocks = []
    pattern = r'```[\s\S]*?```|`[^`]+`'

    def replace_code(match):
        code_blocks.append(match.group(0))
        return f"__CODE_BLOCK_{len(code_blocks) - 1}__"

    cleaned_text = re.sub(pattern, replace_code, content)
    return cleaned_text, code_blocks


def restore_code_blocks(text: str, code_blocks: List[str]) -> str:
    """
    플레이스홀더를 원래 코드 블록으로 복원
    """
    for i, block in enumerate(code_blocks):
        text = text.replace(f"__CODE_BLOCK_{i}__", block)
    return text


def preserve_links(content: str) -> tuple[str, List[str]]:
    """
    마크다운 링크를 보존
    """
    links = []
    pattern = r'\[([^\]]+)\]\(([^)]+)\)'

    def replace_link(match):
        links.append((match.group(1), match.group(2)))
        return f"__LINK_{len(links) - 1}__"

    cleaned_text = re.sub(pattern, replace_link, content)
    return cleaned_text, links


def restore_links(text: str, links: List[tuple]) -> str:
    """
    링크 복원
    """
    for i, (label, url) in enumerate(links):
        # 레이블은 번역되었을 수 있으므로 주변 텍스트에서 찾아서 복원
        text = text.replace(f"__LINK_{i}__", f"[{label}]({url})")
    return text


def process_page(content: str, use_cache: bool = True) -> str:
    """
    페이지 내용을 전처리하여 번역 준비

    Args:
        content: 원본 마크다운 콘텐츠
        use_cache: 캐시된 번역 사용 여부

    Returns:
        전처리된 콘텐츠
    """
    # 코드 블록 보호
    text, code_blocks = extract_code_blocks(content)

    # 링크 보호
    text, links = preserve_links(text)

    # 캐시된 용어 적용
    if use_cache:
        for en, ko in TRANSLATION_CACHE.items():
            # 단어 경계를 고려하여 치환 (부분 일치 방지)
            text = re.sub(rf'\b{re.escape(en)}\b', ko, text)

    # 코드 및 링크 복원
    text = restore_links(text, links)
    text = restore_code_blocks(text, code_blocks)

    return text


def load_pages(input_file: str) -> List[Dict]:
    """
    입력 JSON 파일에서 페이지 정보 로드

    Expected format:
    [
        {
            "url": "https://...",
            "title": "Page Title",
            "content": "Markdown content...",
            "path": "getting-started/installation"
        },
        ...
    ]
    """
    with open(input_file, 'r', encoding='utf-8') as f:
        return json.load(f)


def save_translated_page(output_dir: str, page: Dict):
    """
    번역된 페이지를 파일로 저장
    """
    output_path = Path(output_dir) / f"{page['path']}.mdx"
    output_path.parent.mkdir(parents=True, exist_ok=True)

    with open(output_path, 'w', encoding='utf-8') as f:
        f.write(page['translated_content'])

    print(f"✓ Saved: {output_path}")


def generate_meta_json(pages: List[Dict], output_dir: str):
    """
    _meta.json 파일들을 생성
    """
    # 디렉토리별 페이지 그룹화
    dir_pages: Dict[str, List[Dict]] = {}

    for page in pages:
        path_parts = page['path'].split('/')
        if len(path_parts) > 1:
            dir_name = '/'.join(path_parts[:-1])
        else:
            dir_name = ''

        if dir_name not in dir_pages:
            dir_pages[dir_name] = []
        dir_pages[dir_name].append(page)

    # 각 디렉토리에 _meta.json 생성
    for dir_name, pages_list in dir_pages.items():
        meta = {}

        for page in pages_list:
            file_name = page['path'].split('/')[-1]
            title = page.get('translated_title', page['title'])

            # 하위 페이지가 있는지 확인
            has_children = any(
                p['path'].startswith(f"{page['path']}/")
                for p in pages
            )

            if has_children:
                meta[file_name] = {
                    "title": title,
                    "type": "menu"
                }
            else:
                meta[file_name] = title

        # _meta.json 저장
        meta_path = Path(output_dir) / dir_name / '_meta.json'
        meta_path.parent.mkdir(parents=True, exist_ok=True)

        with open(meta_path, 'w', encoding='utf-8') as f:
            json.dump(meta, f, ensure_ascii=False, indent=2)

        print(f"✓ Generated: {meta_path}")


def main():
    if len(sys.argv) < 3:
        print("Usage: python3 batch_translate.py --input <input.json> --output <output_dir>")
        print("\nInput JSON format:")
        print('[')
        print('  {')
        print('    "url": "https://example.com/docs/page",')
        print('    "title": "Page Title",')
        print('    "content": "Markdown content...",')
        print('    "path": "docs/page"')
        print('  }')
        print(']')
        sys.exit(1)

    # 인자 파싱
    args = sys.argv[1:]
    input_file = None
    output_dir = None

    for i, arg in enumerate(args):
        if arg == '--input' and i + 1 < len(args):
            input_file = args[i + 1]
        elif arg == '--output' and i + 1 < len(args):
            output_dir = args[i + 1]

    if not input_file or not output_dir:
        print("Error: --input and --output are required")
        sys.exit(1)

    # 페이지 로드
    print(f"Loading pages from {input_file}...")
    pages = load_pages(input_file)
    print(f"Loaded {len(pages)} pages")

    # 번역된 페이지 저장
    print(f"\nSaving translated pages to {output_dir}/...")
    for page in pages:
        # 실제 번역은 Claude가 수행하므로 여기서는 구조만 생성
        # 'translated_content' 필드가 있다고 가정
        if 'translated_content' in page:
            save_translated_page(output_dir, page)

    # _meta.json 생성
    print("\nGenerating _meta.json files...")
    generate_meta_json(pages, output_dir)

    print("\n✅ Batch translation complete!")
    print(f"Output directory: {output_dir}")


if __name__ == "__main__":
    main()
