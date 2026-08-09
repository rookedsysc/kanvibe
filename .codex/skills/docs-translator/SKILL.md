---
name: docs-translator
description: Crawl documentation websites, translate all pages to Korean, and structure them in Nextra format with auto-generated meta.json files. Use this skill when users want to translate entire documentation sites to Korean while preserving navigation structure.
---

# Docs Translator

## Overview

This skill enables crawling documentation websites, translating all content to Korean, and organizing the output in Nextra documentation format with proper meta.json files for navigation.

**MDX Writing Rules**: Follow `@.claude/core/mdx.md` for special character escaping (`\<`, `\{`), bold/italic formatting, and Mermaid syntax.

## Workflow

**IMPORTANT**: This skill operates FULLY AUTONOMOUSLY. Do NOT ask the user for confirmation or choices at any step. Complete the entire translation process automatically from start to finish.

### Step 1: Gather Documentation URL

The user provides the documentation website URL to translate.

Example prompts that trigger this skill:
- "Translate the React documentation to Korean"
- "I want to create Korean version of Next.js docs"
- "Convert this docs site to Korean in Nextra format: https://..."

### Playwright Configuration Recommendations

**Headless Mode**: Playwright는 기본적으로 headless mode로 동작하도록 설정하는 것을 권장합니다. 이를 통해 리소스를 절약하고 더 빠른 크롤링이 가능합니다.

- **Headless mode 사용**: 대부분의 문서 크롤링 작업에서 UI를 표시할 필요가 없으므로 headless mode를 사용하세요.
- **Debugging이 필요한 경우**: 링크 추출이 실패하거나 문제가 발생한 경우에만 headed mode로 전환하여 페이지 상태를 직접 확인하세요.
- **성능 최적화**: Headless mode는 메모리 사용량을 줄이고 크롤링 속도를 향상시킵니다.

### Step 2: Collect ALL Page URLs (Complete Before Translation)

**CRITICAL**: You MUST collect the complete list of all page URLs before starting any translation. Do NOT proceed to Step 3 until ALL URLs are collected.

You have TWO options for collecting URLs:

#### Option A: Python Crawler (Recommended for Static Sites)

Use the `crawler.py` script for simple, static documentation sites:

**Prerequisites:**
```bash
pip install -r .claude/skills/docs-translator/requirements.txt
```

**Usage:**
```bash
python3 .claude/skills/docs-translator/scripts/crawler.py <start_url> \
  --max-pages 200 \
  --output translated-docs/all-pages.json \
  --delay 1.0
```

**Advantages:**
- Simple and fast for static HTML
- Automatically handles diverse navigation patterns:
  - AWS docs (awsui classes)
  - MySQL docs (docs-sidebar structures)
  - Standard nav/aside/sidebar elements
  - Nextra/Docusaurus structures
- Built-in rate limiting and error handling
- Saves results directly to JSON

**Limitations:**
- Cannot handle JavaScript-rendered content
- Cannot click to expand collapsed menus
- Best for sites where all links are visible in HTML source

#### Option B: Playwright MCP (For Dynamic Sites)

Use Playwright MCP tools to navigate the documentation site and extract ALL links:

1. **Navigate to the site** using `mcp__playwright__browser_navigate`
   ```
   mcp__playwright__browser_navigate(url: documentation_url)
   ```

2. **Take initial snapshot** to understand the structure
   ```
   mcp__playwright__browser_snapshot()
   ```

3. **Expand ALL collapsible menus recursively** - Documentation sites often have collapsed navigation
   - Look for ALL expandable sidebar elements (buttons with aria-expanded, disclosure widgets)
   - Click EVERY expandable element to reveal hidden links
   - **Keep expanding until NO MORE collapsible elements remain**
   - Use JavaScript evaluation to automate this comprehensively:
     ```javascript
     async function expandAllAndExtractLinks() {
       const baseUrl = window.location.origin;
       const currentPath = window.location.pathname;

       // 1단계: 모든 접기/펼치기 요소 확장
       async function expandAll() {
         let totalExpanded = 0;
         let iteration = 0;
         const maxIterations = 10; // 무한 루프 방지

         while (iteration < maxIterations) {
           // 다양한 확장 가능한 요소 선택자
           const selectors = [
             'button[aria-expanded="false"]',
             'a.expandable:not(.expanded)',
             'a.collapsed',
             'summary:not([open])',
             '[data-state="closed"]',
             '[data-expanded="false"]',
             '.disclosure:not(.open)',
             // AWS 특수 케이스
             'button[class*="expandable"]',
             // 일반적인 토글 버튼
             'button[aria-label*="expand" i]',
             'button[aria-label*="펼치기"]'
           ];

           let expandedThisRound = 0;

           for (const selector of selectors) {
             const elements = document.querySelectorAll(selector);
             for (const el of elements) {
               try {
                 el.click();
                 expandedThisRound++;
                 await new Promise(r => setTimeout(r, 50));
               } catch (e) {
                 // 클릭 불가능한 요소는 무시
               }
             }
           }

           if (expandedThisRound === 0) break;
           totalExpanded += expandedThisRound;
           iteration++;

           // DOM 업데이트 대기
           await new Promise(r => setTimeout(r, 200));
         }

         return totalExpanded;
       }

       const expandedCount = await expandAll();

       // 2단계: 네비게이션 영역에서 링크 추출
       function extractLinks() {
         const links = new Set();

         // 네비게이션 영역 우선 탐색
         const navSelectors = [
           'nav',
           '[role="navigation"]',
           'aside',
           '[class*="sidebar"]',
           '[class*="nav"]',
           '[class*="menu"]',
           '[class*="toc"]',
           // AWS 특수 케이스
           '[class*="awsui"]',
           // MySQL 특수 케이스
           '[class*="docs-sidebar"]'
         ];

         let navContainer = null;
         for (const selector of navSelectors) {
           navContainer = document.querySelector(selector);
           if (navContainer) break;
         }

         const container = navContainer || document.body;

         // 모든 링크 추출
         const anchors = container.querySelectorAll('a[href]');

         for (const anchor of anchors) {
           const href = anchor.getAttribute('href');
           if (!href) continue;

           // 필터링 조건
           // 외부 링크 제외
           if (href.startsWith('http') && !href.startsWith(baseUrl)) continue;
           // 앵커만 있는 링크 제외
           if (href.startsWith('#')) continue;
           // mailto, tel 등 제외
           if (href.startsWith('mailto:') || href.startsWith('tel:')) continue;
           // 소셜 미디어, GitHub 등 제외
           if (href.includes('github.com') || href.includes('twitter.com') ||
               href.includes('facebook.com') || href.includes('discord')) continue;

           // 절대 URL로 변환
           let absoluteUrl = href;
           if (href.startsWith('/')) {
             absoluteUrl = baseUrl + href;
           } else if (!href.startsWith('http')) {
             // 상대 경로 처리
             const base = currentPath.substring(0, currentPath.lastIndexOf('/') + 1);
             absoluteUrl = baseUrl + base + href;
           }

           // 앵커 제거
           absoluteUrl = absoluteUrl.split('#')[0];

           // 중복 제거
           links.add(absoluteUrl);
         }

         return Array.from(links);
       }

       const allLinks = extractLinks();

       return {
         expandedCount: expandedCount,
         totalLinks: allLinks.length,
         links: allLinks,
         baseUrl: baseUrl
       };
     }

     // 실행
     return await expandAllAndExtractLinks();
     ```

   - **지원되는 HTML 구조**:
     - AWS 문서: `<div class="awsui_card-inner_..."><a>` 구조
     - MySQL 문서: `<div class="docs-sidebar-nav-link"><a>` 및 `<a class="expandable">` 토글
     - Nextra/Docusaurus: 표준 `<nav>` 및 `summary/details` 구조
     - 일반적인 사이드바: `[aria-expanded]`, `[data-state]` 속성

4. **Process the extracted links** from the combined function above
   - The `expandAllAndExtractLinks()` function returns all links automatically
   - Links are already filtered (no external links, social media, or anchors)
   - Build a hierarchy map by analyzing URL structure:
     ```javascript
     // Example: Process the result
     const result = await expandAllAndExtractLinks();
     const pages = result.links.map(url => {
       const path = url.replace(result.baseUrl, '');
       const parts = path.split('/').filter(p => p);
       return {
         url: url,
         path: path,
         depth: parts.length,
         // Title will be fetched in translation step
       };
     });
     ```
   - Save to JSON file: `translated-docs/all-pages.json`
   - Use Playwright's evaluate tool with the comprehensive JavaScript code above

5. **Verify completeness**
   - Count total pages discovered
   - Detect URL patterns to understand the path structure
   - Example: `/docs/getting-started` → `docs/getting-started.mdx`
   - Example: `/guide/introduction` → `guide/introduction.md`

### Step 3: Parallel Translation Using Task Agents

**CRITICAL**: Only start this step after Step 2 is 100% complete.

1. **Load the complete URL list** from `translated-docs/all-pages.json`

2. **Divide pages into groups** for parallel processing
   - Split all pages into 4-6 equal groups
   - Save each group to `translated-docs/group-{N}.json`

3. **Launch parallel Task agents** - Use ONE message with MULTIPLE Task tool calls
   ```
   Launch 4-6 Task agents in PARALLEL (single message, multiple tool uses)
   Each agent processes one group autonomously
   ```

4. **Each agent task instructions**:
   - Read assigned group JSON file
   - For EACH page in the group:
     * Fetch content using WebFetch
     * Translate to Korean (following guidelines below)
     * Save to appropriate file path
   - Report completion with success/failure counts

5. **Wait for ALL agents to complete** before proceeding to Step 4

### Translation Guidelines (For Task Agents)

When translating each page:

1. **Add original URL at the top** of EVERY translated file:
   ```markdown
   > **원본 문서**: https://docs.example.com/path/to/page.html

   # Page Title (페이지 제목)
   ```

2. **Fetch page content** using WebFetch
   ```
   WebFetch(url, prompt: "Extract the main documentation content excluding headers, footers, and navigation. Return as markdown.")
   ```

3. **Translate to Korean** while preserving:
   - Code blocks (do NOT translate code)
   - Technical terms (keep in English with Korean explanation in parentheses if needed)
   - URLs and links
   - Markdown formatting

4. **Translation style**:
   - Use formal Korean (존댓말)
   - Keep framework/library names in English (e.g., "React", "Next.js")
   - Translate UI element names (e.g., "button" → "버튼")
   - Keep function/variable names untranslated
   - Add Korean equivalents for technical terms when first mentioned

Example translation with original URL:
```markdown
> **원본 문서**: https://react.dev/learn/getting-started

# Getting Started (시작하기)

React는 사용자 인터페이스를 구축하기 위한 JavaScript 라이브러리입니다.

```jsx
function Welcome() {
  return <h1>Hello, World!</h1>;
}
```

`Welcome` 컴포넌트는 간단한 greeting을 렌더링합니다.
```

### Step 4: Structure in Nextra Format

Organize translated content following Nextra conventions:

1. **Create directory structure** matching the original site's hierarchy
   ```
   translated-docs/
   ├── index.mdx          (homepage)
   ├── _meta.json         (root navigation)
   ├── getting-started/
   │   ├── _meta.json
   │   ├── installation.mdx
   │   └── quickstart.mdx
   └── guides/
       ├── _meta.json
       ├── basics.mdx
       └── advanced/
           ├── _meta.json
           └── patterns.mdx
   ```

2. **Generate _meta.json files** for each directory

   Format:
   ```json
   {
     "index": "홈",
     "getting-started": "시작하기",
     "guides": {
       "title": "가이드",
       "type": "page"
     },
     "api": {
       "title": "API 레퍼런스",
       "type": "menu"
     }
   }
   ```

   Rules:
   - Filename (without extension) as key
   - Korean translated title as value (string) for simple pages
   - Object with `title` and `type` for sections with children
   - `type`: "page" (single page) | "menu" (has children) | "separator"
   - Maintain order from original navigation

3. **File naming conventions**:
   - Use lowercase with hyphens: `getting-started.mdx`
   - Match original URL slugs when possible
   - Use `.mdx` extension for Nextra compatibility

### Step 5: Finalize and Report

**This step happens AUTOMATICALLY after all Task agents complete.**

1. **Verify all files were created**
   - Count total MDX files in `translated-docs/pages/`
   - Check for any missing pages from the original URL list
   - List any failed translations

2. **Generate _meta.json files** for Nextra navigation
   - Create based on the hierarchy from `all-pages.json`
   - Automatically determine structure without asking user

3. **Generate final summary report** and display to user:
   ```markdown
   # 번역 완료

   - 총 번역된 페이지: X
   - 출력 디렉토리: ./translated-docs
   - 구조:
     - 루트 페이지: Y
     - 중첩 섹션: Z
   - 실패한 페이지: N (있는 경우)

   ## 다음 단계

   1. _meta.json 파일 검토하여 네비게이션 순서 확인
   2. Nextra 설치: `npm install nextra nextra-theme-docs`
   3. next.config.js에서 Nextra 테마 설정
   4. 개발 서버 실행: `npm run dev`
   ```

**IMPORTANT**: Do NOT ask user if they want to continue or make choices. Complete ALL steps automatically.

## Resources

### scripts/

This skill includes Python scripts for crawling and translation:

- `crawler.py` - Flexible documentation site crawler
  - Supports diverse HTML patterns (AWS, MySQL, Nextra, etc.)
  - BFS-based crawling with configurable depth limits
  - Automatic link extraction from nav/sidebar/menu elements
  - Built-in filtering for external links and non-doc resources
  - Rate limiting and timeout handling
  - Outputs JSON with URL, path, depth, and title

  Usage:
  ```bash
  python3 scripts/crawler.py https://docs.example.com \
    --max-pages 200 \
    --output crawled-urls.json \
    --delay 1.0
  ```

- `batch_translate.py` - Handles translation of multiple pages efficiently
  - Groups similar content for consistent terminology
  - Caches translations to avoid re-translating common phrases
  - Preserves code blocks and technical terms

  Usage:
  ```bash
  python3 scripts/batch_translate.py --input pages.json --output translated/
  ```

### references/

- `nextra-structure.md` - Detailed Nextra format specifications and _meta.json examples
- `translation-glossary.md` - Common technical term translations for consistency

## Error Handling

**Site requires authentication**: Inform user that authenticated sites cannot be crawled automatically. Request exported HTML or API access.

**Rate limiting detected**: Add delays between requests (`browser_wait_for` with time parameter). Inform user of extended processing time.

**Translation API errors**: Batch failed pages and retry after delay. Report any pages that couldn't be translated.

**Invalid Nextra structure**: Validate _meta.json format before saving. Fix common issues (missing commas, invalid types).

## Tips

- **NEVER ask user for confirmation** - This skill runs fully autonomously from start to finish
- **Always collect ALL URLs first** before starting any translation work
- **Use parallel Task agents** for maximum efficiency (4-6 agents recommended)
- For large documentation sites (>100 pages), the process may take 10-30 minutes - inform user and continue automatically
- Check for existing translations in the original docs (some sites have i18n already)
- Preserve frontmatter from original MDX files if present
- **Always add original URL** at the top of every translated file
