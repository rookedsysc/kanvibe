# KanVibe 기여 가이드

[EN](../CONTRIBUTING.md) | [ZH](./CONTRIBUTING.zh.md)

KanVibe에 관심을 가져주셔서 감사합니다! 이 가이드가 시작하는 데 도움이 될 것입니다.

---

## 개발 환경 설정

### 사전 요구사항

- Node.js 20+ (`.nvmrc` 참조)
- pnpm
- Docker (PostgreSQL용)
- tmux 또는 zellij (터미널 기능용)

### 시작하기

```bash
# 저장소 클론
git clone https://github.com/rookedsysc/kanvibe.git
cd kanvibe

# 환경변수 복사
cp .env.example .env

# PostgreSQL 시작
docker compose up db -d

# 의존성 설치
pnpm install

# 데이터베이스 마이그레이션 실행
pnpm migration:run

# 개발 서버 시작
pnpm dev
```

브라우저에서 `http://localhost:4885` 접속.

---

## Pull Request 가이드

### 제출 전 확인사항

1. **모든 체크가 통과해야 합니다:**
   ```bash
   pnpm build    # 빌드 성공
   pnpm check    # 타입 체크 통과
   pnpm test     # 테스트 통과
   ```

2. **동작하는 스크린샷 또는 GIF를 첨부하세요.** 기능 동작을 시각적으로 증명하지 않은 PR은 머지되지 않습니다.

3. **기존 코드 스타일을 따르세요.** 프로젝트에서 사용하는 규칙:
   - TypeScript strict 모드
   - Tailwind CSS v4 + 디자인 토큰 (CSS 변수)
   - next-intl로 모든 사용자 텍스트 처리
   - TypeORM migration으로 스키마 변경

### PR 프로세스

1. 저장소를 Fork
2. `main`에서 feature 브랜치 생성
3. 변경 사항 작성
4. 모든 체크 실행 (`pnpm build && pnpm check && pnpm test`)
5. [Conventional Commits](https://www.conventionalcommits.org/) 형식으로 커밋
6. Push 후 Pull Request 생성
7. 동작하는 기능의 스크린샷/GIF 첨부

### 커밋 메시지 형식

```
feat(scope): 새 기능 추가
fix(scope): 특정 버그 수정
docs: 문서 업데이트
refactor(scope): 코드 구조 개선
```

---

## 국제화 (i18n)

모든 사용자 대면 문자열은 번역이 필요합니다. UI 텍스트를 추가하거나 수정할 때:

1. `messages/ko.json`에 키/값 추가
2. `messages/en.json`과 `messages/zh.json`에 번역 추가
3. 컴포넌트에서 `useTranslations` 또는 `getTranslations`로 `t("key")` 사용

---

## 데이터베이스 변경

모든 스키마 변경은 TypeORM migration을 통해 수행합니다:

```bash
# 엔티티 변경으로부터 마이그레이션 생성
pnpm migration:generate -- src/migrations/DescriptiveName

# 마이그레이션 실행
pnpm migration:run
```

절대 `synchronize: true`를 사용하지 마세요. 자세한 마이그레이션 워크플로우는 `CLAUDE.md`를 참조하세요.

---

## 문서 업데이트

사용자 대면 동작에 영향을 주는 변경을 할 때는 해당 문서도 함께 업데이트하세요:

| 변경 내용 | 업데이트 대상 |
|-----------|--------------|
| 기능 또는 UI | `README.md`, `docs/README.ko.md`, `docs/README.zh.md` |
| 기여 프로세스 | `docs/CONTRIBUTING.md`, `docs/CONTRIBUTING.ko.md`, `docs/CONTRIBUTING.zh.md` |
| 환경변수 | `.env.example` + 모든 README 파일 |
| Hook API | 모든 README 파일 (Hook API 섹션) |

세 가지 언어 버전 (EN, KO, ZH)은 항상 함께 업데이트해야 합니다.

---

## 다음 단계 & 로드맵

### Gemini Hooks / Codex Hooks 지원 (개발 중)

현재 KanVibe는 자동 상태 추적을 위해 **Claude Code Hooks**를 지원합니다. **Gemini Hooks**와 **Codex Hooks** 지원은 계획 중이며 개발 진행 중입니다.

멀티 에이전트 hook 지원에 대해 더 좋은 방향이나 아키텍처 아이디어가 있다면, 먼저 [Discussion](https://github.com/rookedsysc/kanvibe/discussions)을 열어주세요. 구현 전에 함께 최적의 방향을 찾고 싶습니다.

### 토큰 사용량 대시보드

다음 주요 기능 목표는 **토큰 사용량 추적 대시보드**입니다 - 태스크와 세션별 AI 에이전트 토큰 소비량을 모니터링합니다.

### 새 기능 제안 방법

중요한 변경이나 새 기능의 경우:
1. [Discussion](https://github.com/rookedsysc/kanvibe/discussions)에서 아이디어를 공유
2. 커뮤니티와 메인테이너의 피드백을 받기
3. 방향이 합의되면 Issue를 생성하고 PR 제출

---

## 라이센스

KanVibe에 기여함으로써, 귀하의 기여가 [AGPL-3.0 라이센스](../LICENSE)에 따라 라이선스됨에 동의합니다.
