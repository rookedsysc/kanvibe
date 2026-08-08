#!/usr/bin/env python3
"""
Documentation site crawler with flexible pattern matching.
Supports various HTML structures including AWS docs, MySQL docs, and standard documentation sites.

Usage:
    python3 crawler.py <start_url> [--max-pages MAX] [--output OUTPUT] [--delay DELAY]

Example:
    python3 crawler.py https://docs.example.com --max-pages 100 --output urls.json
"""

import json
import re
import sys
import time
from collections import deque
from pathlib import Path
from typing import Dict, List, Set
from urllib.parse import urljoin, urlparse, urlunparse

try:
    import requests
    from bs4 import BeautifulSoup
except ImportError:
    print("Error: Required libraries not found.")
    print("Install with: pip install requests beautifulsoup4")
    sys.exit(1)


class DocsCrawler:
    """문서 사이트 크롤러 - 다양한 HTML 패턴 지원"""

    def __init__(
        self,
        start_url: str,
        max_pages: int = 100,
        delay: float = 1.0,
        timeout: int = 10
    ):
        """
        Args:
            start_url: 크롤링 시작 URL
            max_pages: 최대 크롤링 페이지 수
            delay: 요청 간 대기 시간 (초)
            timeout: HTTP 요청 타임아웃 (초)
        """
        self.start_url = start_url
        self.max_pages = max_pages
        self.delay = delay
        self.timeout = timeout

        # URL 정보 추출
        parsed = urlparse(start_url)
        self.base_domain = f"{parsed.scheme}://{parsed.netloc}"
        self.base_path = parsed.path.rstrip('/')

        # 크롤링 상태
        self.visited: Set[str] = set()
        self.to_visit: deque = deque([start_url])
        self.pages: List[Dict] = []

        # HTTP 세션 (연결 재사용)
        self.session = requests.Session()
        self.session.headers.update({
            'User-Agent': 'Mozilla/5.0 (compatible; DocsCrawler/1.0; +educational)'
        })

    def crawl(self) -> List[Dict]:
        """
        BFS 방식으로 문서 사이트 크롤링

        Returns:
            List[Dict]: 발견된 페이지 정보 리스트
            [
                {
                    "url": "https://...",
                    "path": "/docs/page",
                    "depth": 2,
                    "title": "Page Title"
                },
                ...
            ]
        """
        print(f"Starting crawl from: {self.start_url}")
        print(f"Max pages: {self.max_pages}")
        print(f"Delay: {self.delay}s\n")

        depth_map = {self.start_url: 0}

        while self.to_visit and len(self.visited) < self.max_pages:
            url = self.to_visit.popleft()

            # 이미 방문했거나 유효하지 않은 URL 건너뛰기
            if url in self.visited or not self._is_valid_url(url):
                continue

            # 페이지 가져오기 및 처리
            try:
                content, links, title = self._fetch_page(url)

                # 페이지 정보 저장
                page_info = {
                    "url": url,
                    "path": self._url_to_path(url),
                    "depth": depth_map.get(url, 0),
                    "title": title
                }
                self.pages.append(page_info)
                self.visited.add(url)

                print(f"✓ [{len(self.visited)}/{self.max_pages}] {url}")
                print(f"  Title: {title}")
                print(f"  Found {len(links)} links")

                # 발견된 링크를 큐에 추가
                current_depth = depth_map.get(url, 0)
                for link in links:
                    if link not in self.visited and link not in self.to_visit:
                        self.to_visit.append(link)
                        depth_map[link] = current_depth + 1

                # Rate limiting
                time.sleep(self.delay)

            except Exception as e:
                print(f"✗ Error fetching {url}: {e}")
                continue

        print(f"\n✅ Crawl complete: {len(self.visited)} pages")
        return self.pages

    def _fetch_page(self, url: str) -> tuple[str, List[str], str]:
        """
        페이지를 가져와서 링크 추출

        Returns:
            tuple: (content, links, title)
        """
        response = self.session.get(url, timeout=self.timeout)
        response.raise_for_status()

        soup = BeautifulSoup(response.text, 'html.parser')

        # 제목 추출
        title = self._extract_title(soup)

        # 링크 추출 (다양한 패턴 지원)
        links = self._extract_links(soup, url)

        return response.text, links, title

    def _extract_title(self, soup: BeautifulSoup) -> str:
        """
        페이지 제목 추출

        우선순위:
        1. <h1> 태그
        2. <title> 태그
        3. og:title 메타 태그
        """
        # h1 태그
        h1 = soup.find('h1')
        if h1:
            return h1.get_text(strip=True)

        # title 태그
        title = soup.find('title')
        if title:
            return title.get_text(strip=True)

        # og:title
        og_title = soup.find('meta', property='og:title')
        if og_title and og_title.get('content'):
            return og_title['content']

        return "Untitled"

    def _extract_links(self, soup: BeautifulSoup, base_url: str) -> List[str]:
        """
        페이지에서 문서 링크 추출 (다양한 패턴 지원)

        지원하는 패턴:
        1. AWS 문서: awsui_card, awsui_link 클래스
        2. MySQL 문서: docs-sidebar-nav-link, expandable
        3. 표준 네비게이션: nav, aside, sidebar, menu, toc
        4. 일반 링크 (필터링 적용)
        """
        links = set()

        # 패턴 1: 네비게이션 영역 우선 탐색
        nav_selectors = [
            'nav',
            '[role="navigation"]',
            'aside',
            '[class*="sidebar"]',
            '[class*="nav"]',
            '[class*="menu"]',
            '[class*="toc"]',
            # AWS 특수 케이스
            '[class*="awsui"]',
            # MySQL 특수 케이스
            '[class*="docs-sidebar"]',
            # Nextra/Docusaurus
            '[class*="nextra"]',
            '[class*="docusaurus"]',
        ]

        # 네비게이션 영역 찾기
        nav_container = None
        for selector in nav_selectors:
            nav_container = soup.select_one(selector)
            if nav_container:
                break

        # 네비게이션이 없으면 본문 검색
        search_area = nav_container if nav_container else soup

        # 패턴 2: 모든 a 태그 추출
        anchors = search_area.find_all('a', href=True)

        for anchor in anchors:
            href = anchor.get('href', '').strip()
            if not href:
                continue

            # 절대 URL로 변환
            absolute_url = urljoin(base_url, href)

            # 링크 유효성 검사
            if self._is_valid_link(absolute_url, href):
                # 앵커 제거
                clean_url = self._remove_anchor(absolute_url)
                links.add(clean_url)

        return list(links)

    def _is_valid_link(self, absolute_url: str, original_href: str) -> bool:
        """
        링크 유효성 검사

        제외 대상:
        - 앵커만 있는 링크 (#section)
        - 외부 도메인
        - mailto:, javascript:, tel: 등
        - 소셜 미디어, GitHub, Discord 등
        - 파일 다운로드 (선택적으로 PDF는 포함 가능)
        """
        # 앵커만 있는 링크
        if original_href.startswith('#'):
            return False

        # 특수 프로토콜
        if any(original_href.startswith(proto) for proto in ['mailto:', 'tel:', 'javascript:', 'data:']):
            return False

        parsed = urlparse(absolute_url)

        # 외부 도메인 (다른 호스트)
        if parsed.netloc and parsed.netloc != urlparse(self.base_domain).netloc:
            return False

        # 소셜 미디어 및 개발 플랫폼 제외
        excluded_patterns = [
            'github.com',
            'twitter.com',
            'x.com',
            'facebook.com',
            'linkedin.com',
            'discord.com',
            'youtube.com',
            'reddit.com',
        ]
        if any(pattern in absolute_url for pattern in excluded_patterns):
            return False

        # 파일 확장자 필터 (문서는 허용, 바이너리는 제외)
        excluded_extensions = [
            '.zip', '.tar', '.gz', '.7z',
            '.exe', '.dmg', '.pkg',
            '.jpg', '.jpeg', '.png', '.gif', '.svg', '.ico',
            '.mp4', '.mp3', '.avi', '.mov',
        ]
        if any(absolute_url.lower().endswith(ext) for ext in excluded_extensions):
            return False

        return True

    def _is_valid_url(self, url: str) -> bool:
        """URL이 크롤링 범위 내에 있는지 확인"""
        parsed = urlparse(url)

        # 같은 도메인
        if parsed.netloc != urlparse(self.base_domain).netloc:
            return False

        # 같은 경로 하위 (선택적)
        # 예: /docs/로 시작한 경우 /docs/ 하위만 크롤링
        # 이 제약을 제거하려면 return True
        if self.base_path and not parsed.path.startswith(self.base_path):
            return False

        return True

    def _remove_anchor(self, url: str) -> str:
        """URL에서 앵커(#) 제거"""
        parsed = urlparse(url)
        return urlunparse(parsed._replace(fragment=''))

    def _url_to_path(self, url: str) -> str:
        """
        URL을 파일 경로로 변환

        예:
            https://docs.example.com/guide/intro.html -> /guide/intro
            https://docs.example.com/api/ -> /api/index
        """
        parsed = urlparse(url)
        path = parsed.path.rstrip('/')

        # .html, .htm 확장자 제거
        path = re.sub(r'\.(html?|php|asp)$', '', path, flags=re.IGNORECASE)

        # 빈 경로는 /index
        if not path or path == '/':
            path = '/index'

        return path

    def save_results(self, output_file: str):
        """
        크롤링 결과를 JSON 파일로 저장

        Args:
            output_file: 출력 파일 경로
        """
        output_path = Path(output_file)
        output_path.parent.mkdir(parents=True, exist_ok=True)

        with open(output_path, 'w', encoding='utf-8') as f:
            json.dump(self.pages, f, ensure_ascii=False, indent=2)

        print(f"\n💾 Saved results to: {output_file}")
        print(f"Total pages: {len(self.pages)}")


def main():
    """CLI 진입점"""
    if len(sys.argv) < 2:
        print("Usage: python3 crawler.py <start_url> [OPTIONS]")
        print("\nOptions:")
        print("  --max-pages MAX    Maximum pages to crawl (default: 100)")
        print("  --output FILE      Output JSON file (default: crawled-urls.json)")
        print("  --delay SECONDS    Delay between requests (default: 1.0)")
        print("\nExample:")
        print("  python3 crawler.py https://docs.example.com --max-pages 200 --output docs.json")
        sys.exit(1)

    # 인자 파싱
    start_url = sys.argv[1]
    max_pages = 100
    output_file = "crawled-urls.json"
    delay = 1.0

    args = sys.argv[2:]
    for i, arg in enumerate(args):
        if arg == '--max-pages' and i + 1 < len(args):
            max_pages = int(args[i + 1])
        elif arg == '--output' and i + 1 < len(args):
            output_file = args[i + 1]
        elif arg == '--delay' and i + 1 < len(args):
            delay = float(args[i + 1])

    # 크롤러 실행
    try:
        crawler = DocsCrawler(
            start_url=start_url,
            max_pages=max_pages,
            delay=delay
        )

        pages = crawler.crawl()
        crawler.save_results(output_file)

        # 통계 출력
        print("\n📊 Statistics:")
        print(f"  Total URLs: {len(pages)}")
        if pages:
            depths = [p['depth'] for p in pages]
            print(f"  Max depth: {max(depths)}")
            print(f"  Avg depth: {sum(depths) / len(depths):.1f}")

    except KeyboardInterrupt:
        print("\n\n⚠️  Crawl interrupted by user")
        if crawler.pages:
            print(f"Saving {len(crawler.pages)} pages collected so far...")
            crawler.save_results(output_file)
        sys.exit(0)
    except Exception as e:
        print(f"\n❌ Error: {e}")
        sys.exit(1)


if __name__ == "__main__":
    main()
