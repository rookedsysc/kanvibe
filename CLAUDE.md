# KanVibe

AI 에이전트 작업 관리 Kanban 보드.

## Tech Stack

- Next.js 16 (App Router) + React 19 + TypeScript
- Tailwind CSS v4 (CSS Variables + @theme inline)
- next-intl (i18n)
- TypeORM + PostgreSQL
- @hello-pangea/dnd (drag & drop)

## Design System

디자인 토큰은 `prd/design-system.json`에 정의되고 `src/app/globals.css`에 CSS 변수로 선언된다.

### 토큰 사용법

Tailwind 클래스에서 CSS 변수 토큰을 직접 사용한다:

- 배경: `bg-bg-page`, `bg-bg-surface`, `bg-brand-primary`
- 텍스트: `text-text-primary`, `text-text-secondary`, `text-text-muted`
- 보더: `border-border-default`, `border-border-strong`
- 상태: `bg-status-todo`, `bg-status-progress`, `bg-status-review`, `bg-status-done`
- 태그: `bg-tag-claude-bg text-tag-claude-text`, `bg-tag-branch-bg text-tag-branch-text`
- 그림자: `shadow-sm`, `shadow-md`, `shadow-lg`

### 색상 기조

Google 브랜드 컬러 (#4285F4, #EA4335, #FBBC05, #34A853) 기반 + 블랙&화이트 라이트 테마.

### 새 토큰 추가 시

1. `prd/design-system.json`에 토큰 정의
2. `src/app/globals.css`의 `:root`에 CSS 변수 추가
3. `@theme inline` 블록에 Tailwind 등록

## Database Migration

TypeORM migration 기반 스키마 관리. `synchronize`는 비활성화되어 있으며, 모든 스키마 변경은 migration으로 수행한다.

### 파일 구조

- `src/lib/typeorm-cli.config.ts` — CLI 전용 DataSource 설정 (migration generate/run/revert)
- `src/lib/database.ts` — 앱 런타임 DataSource 설정 (`migrationsRun: true`로 자동 실행)
- `src/migrations/*.ts` — 마이그레이션 파일 (타임스탬프 기반 정렬)

### 스키마 변경 시

1. `src/entities/`에서 엔티티 수정
2. `npm run migration:generate -- src/migrations/DescriptiveName` 실행 (DB 실행 상태 필요)
3. 생성된 마이그레이션 파일의 SQL 검토
4. `src/lib/database.ts`의 `migrations` 배열에 새 마이그레이션 클래스 import 추가
5. `npm run migration:run`으로 적용

### 주요 명령어

```bash
# 엔티티 변경 기반 마이그레이션 생성
npm run migration:generate -- src/migrations/AddColumnName

# 마이그레이션 실행
npm run migration:run

# 마지막 마이그레이션 롤백
npm run migration:revert
```

### 주의사항

- `synchronize: true` 사용 금지 (프로덕션 데이터 손실 위험)
- 마이그레이션 파일은 생성 후 수정하지 않는다 (이미 실행된 환경과 불일치 발생)
- 새 마이그레이션 생성 시 반드시 `database.ts`의 `migrations` 배열에 추가한다
- 기존 DB 전환: `migrations` 테이블에 초기 마이그레이션 레코드를 수동 삽입한다

```sql
INSERT INTO migrations (timestamp, name) VALUES (1770854400000, 'InitialSchema1770854400000');
```

## 국제화 (i18n)

next-intl 기반. 지원 언어: 한국어(ko), 영어(en), 중국어(zh). 기본 언어: ko.

### 파일 구조

- `messages/{locale}.json` — 번역 파일
- `src/i18n/routing.ts` — locale 목록, 기본 locale 정의
- `src/i18n/request.ts` — 서버 요청별 locale/메시지 로딩
- `src/i18n/navigation.ts` — locale-aware Link, redirect, usePathname, useRouter

### 번역 사용법

- 클라이언트 컴포넌트: `const t = useTranslations("namespace")`
- 서버 컴포넌트: `const t = await getTranslations("namespace")`
- 네비게이션: `import { Link } from "@/i18n/navigation"` (next/link 대신)

### 번역 키 추가 시

1. `messages/ko.json`에 한국어 키/값 추가
2. `messages/en.json`, `messages/zh.json`에 동일 키로 번역 추가
3. 컴포넌트에서 `t("key")` 또는 `t("namespace.key")`로 사용
