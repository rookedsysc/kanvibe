# Nextra 구조 가이드

## _meta.json 형식

Nextra는 `_meta.json` 파일을 사용하여 사이드바 네비게이션과 페이지 순서를 정의합니다.

### 기본 구조

```json
{
  "index": "홈",
  "getting-started": "시작하기",
  "features": "기능"
}
```

- **키(key)**: 파일명 (확장자 제외)
- **값(value)**: 네비게이션에 표시될 제목

### 고급 구조

```json
{
  "index": "홈",
  "docs": {
    "title": "문서",
    "type": "page"
  },
  "guides": {
    "title": "가이드",
    "type": "menu",
    "collapsed": false
  },
  "--": {
    "type": "separator"
  },
  "api": {
    "title": "API 레퍼런스",
    "type": "page"
  },
  "link": {
    "title": "GitHub →",
    "href": "https://github.com/..."
  }
}
```

### type 옵션

- `"page"`: 단일 페이지 (하위 항목 없음)
- `"menu"`: 하위 항목이 있는 폴더
- `"separator"`: 네비게이션 구분선
- 지정하지 않으면 기본값은 파일/폴더에 따라 자동 결정

### 추가 옵션

```json
{
  "advanced": {
    "title": "고급 기능",
    "type": "menu",
    "collapsed": true,
    "display": "hidden",
    "theme": {
      "breadcrumb": false,
      "sidebar": true
    }
  }
}
```

- `collapsed`: 기본적으로 접힌 상태로 표시 (기본값: false)
- `display`: "hidden"이면 사이드바에서 숨김
- `theme`: 페이지별 테마 설정

### 중첩 구조 예시

```
docs/
├── _meta.json
├── index.mdx
├── getting-started/
│   ├── _meta.json
│   ├── installation.mdx
│   └── quickstart.mdx
└── guides/
    ├── _meta.json
    ├── basics.mdx
    └── advanced/
        ├── _meta.json
        └── optimization.mdx
```

**docs/_meta.json**:
```json
{
  "index": "홈",
  "getting-started": "시작하기",
  "guides": "가이드"
}
```

**docs/getting-started/_meta.json**:
```json
{
  "installation": "설치",
  "quickstart": "빠른 시작"
}
```

**docs/guides/_meta.json**:
```json
{
  "basics": "기초",
  "advanced": "고급"
}
```

**docs/guides/advanced/_meta.json**:
```json
{
  "optimization": "최적화"
}
```

## 파일명 규칙

- 소문자와 하이픈 사용: `getting-started.mdx`
- 공백 대신 하이픈: `api-reference.mdx`
- 특수 문자 제거: `whats-new.mdx` (not `what's-new.mdx`)
- URL 친화적: 파일명이 URL path가 됨

## MDX 프론트매터

각 MDX 파일에 메타데이터 추가 가능:

```mdx
---
title: Getting Started
description: Learn how to get started with our framework
---

# Getting Started

Content here...
```

프론트매터 필드:
- `title`: 페이지 제목 (브라우저 탭, SEO)
- `description`: 페이지 설명 (SEO)
- `---`: 구분자로 필수

## 자동 생성 가이드

URL 구조에서 _meta.json 생성 로직:

1. URL path를 분석하여 계층 구조 파악
   - `/docs/getting-started/installation` → `docs/getting-started/_meta.json`에 `installation` 항목 추가

2. 각 디렉토리마다 _meta.json 생성
   - 해당 디렉토리의 직접 자식만 포함
   - 순서는 원본 사이트의 네비게이션 순서 유지

3. 페이지 제목 번역
   - URL slug를 사람이 읽을 수 있는 한국어로 변환
   - `getting-started` → "시작하기"
   - `api-reference` → "API 레퍼런스"

4. type 자동 결정
   - 하위 페이지가 있으면 `type: "menu"`
   - 없으면 단순 문자열 또는 `type: "page"`

## 검증 규칙

_meta.json 파일은 반드시:
- 유효한 JSON 형식
- 존재하는 파일/폴더만 참조
- 중복 키 없음
- 순환 참조 없음
