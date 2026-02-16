# Header Logo & Favicon Setup

## Business Goal
KanVibe 브랜드 아이덴티티 강화를 위해 헤더에 로고 이미지를 표시하고, 웹사이트 파비콘을 커스텀 로고로 설정한다.

## Scope
- **In Scope**: 헤더 로고 이미지 배치, 파비콘 교체, 기존 favicon.ico 제거
- **Out of Scope**: 다크모드 대응, apple-touch-icon, 반응형 로고 크기 변경

## Codebase Analysis Summary
- 헤더는 `src/components/Board.tsx`의 `<header>` 태그 내에 `<h1>` 텍스트로 구현
- 파비콘은 `src/app/favicon.ico`에 기본 Next.js 아이콘 사용 중
- 이미지 파일은 프로젝트 루트에 `kanvibe-left-header.png`, `kanvibe-logo.png` 존재

### Relevant Files
| File | Role | Action |
|------|------|--------|
| `kanvibe-left-header.png` | 헤더 로고 원본 | Move to `public/` |
| `kanvibe-logo.png` | 파비콘 원본 | Move to `src/app/icon.png` |
| `src/components/Board.tsx` | 메인 보드 헤더 | Modify (h1 → Image) |
| `src/app/favicon.ico` | 기존 파비콘 | Delete |

### Conventions to Follow
| Convention | Source | Rule |
|-----------|--------|------|
| Next.js Image | Framework | `next/image` 컴포넌트 사용 |
| File-based metadata | Next.js App Router | `src/app/icon.png` 컨벤션으로 파비콘 자동 인식 |

## Implementation Todos

### Todo 1: Move image files to correct locations
- **Priority**: 1
- **Dependencies**: none
- **Goal**: 이미지 파일을 Next.js가 인식할 수 있는 위치로 이동
- **Work**:
  - `kanvibe-left-header.png` → `public/kanvibe-left-header.png` 복사
  - `kanvibe-logo.png` → `src/app/icon.png` 복사
  - `src/app/favicon.ico` 삭제
- **Convention Notes**: Next.js App Router file convention (`icon.png` in app dir)
- **Verification**: 파일 존재 확인
- **Exit Criteria**: `public/kanvibe-left-header.png`, `src/app/icon.png` 존재, `favicon.ico` 제거
- **Status**: pending

### Todo 2: Replace header text with logo image
- **Priority**: 2
- **Dependencies**: Todo 1
- **Goal**: Board 헤더의 텍스트를 이미지 로고로 교체
- **Work**:
  - `src/components/Board.tsx` line 345의 `<h1>` 태그를 `next/image` `<Image>` 컴포넌트로 교체
  - `src="/kanvibe-left-header.png"`, 적절한 height/width, alt text 설정
- **Convention Notes**: Next.js `<Image>` 컴포넌트 사용, alt 텍스트 필수
- **Verification**: `npm run build` 성공
- **Exit Criteria**: 헤더에 로고 이미지가 표시되고 빌드 에러 없음
- **Status**: pending

## Verification Strategy
- `npm run build` 성공 확인

## Progress Tracking
- Total Todos: 2
- Completed: 2
- Status: Execution complete

## Change Log
- 2026-02-16: Plan created
