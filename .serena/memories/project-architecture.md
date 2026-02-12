# KanVibe Project Architecture

## Tech Stack
- Next.js 16 (App Router) + React 19 + TypeScript 5
- Tailwind CSS v4 (CSS Variables + @theme inline in globals.css)
- next-intl for i18n (ko, en, zh) with locale-based routing (/[locale]/...)
- TypeORM + PostgreSQL
- @hello-pangea/dnd for drag & drop

## Design System
- Tokens defined in `prd/design-system.json`
- CSS variables in `src/app/globals.css` (:root + @theme inline)
- Google brand colors (#4285F4, #EA4335, #FBBC05, #34A853) + black & white base
- Light theme only
- Naming: `--color-{category}-{name}` (brand, bg, text, border, status, tag)
- Tailwind classes: `bg-bg-page`, `text-text-primary`, `border-border-default`, `bg-status-todo`, `bg-tag-claude-bg`

## i18n Structure
- `src/i18n/routing.ts` — locale config (ko default)
- `src/i18n/request.ts` — server message loading
- `src/i18n/navigation.ts` — locale-aware Link, redirect, usePathname, useRouter
- `messages/{locale}.json` — translation files
- Client: `useTranslations("namespace")`, Server: `getTranslations("namespace")`
- Middleware: combined locale routing + auth check in `src/middleware.ts`

## Key Files
- Root layout: `src/app/layout.tsx` (thin wrapper, passes children through)
- Locale layout: `src/app/[locale]/layout.tsx` (NextIntlClientProvider, Inter font, globals.css)
- Pages: `src/app/[locale]/page.tsx`, `src/app/[locale]/login/page.tsx`, `src/app/[locale]/task/[id]/page.tsx`
- Components: `src/components/Board.tsx` (main), Column, TaskCard, CreateTaskModal, BranchTaskModal, ProjectSettings, TaskContextMenu, TaskStatusBadge, Terminal, TerminalLoader

## Database Migration
- TypeORM migration 기반 스키마 관리 (`synchronize: false`)
- CLI 설정: `src/lib/typeorm-cli.config.ts` (tsx 기반, 상대 경로 사용)
- 앱 설정: `src/lib/database.ts` (`migrationsRun: true`로 자동 실행)
- 마이그레이션 파일: `src/migrations/*.ts`
- 새 마이그레이션 생성 시 `database.ts`의 `migrations` 배열에 import 추가 필수
- 스크립트: `migration:generate`, `migration:run`, `migration:revert`

## Conventions
- Korean comments (CODE_PRINCIPLES.md)
- UTF-8 heredoc for Korean content files (FILE_WRITE_PRINCIPLES.md)
- `@/*` path alias → `./src/*`
- "use client" directive for client components
