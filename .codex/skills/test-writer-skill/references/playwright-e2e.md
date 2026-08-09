# Playwright E2E 테스트 가이드

## 기본 원칙

### Given-When-Then 패턴

모든 Playwright 테스트는 Given-When-Then 구조를 따르며, 주석으로 각 섹션을 구분한다.

### 비즈니스 의미가 담긴 테스트 설명

- 기술적 구현이 아닌 비즈니스 시나리오를 설명한다
- 나쁜 예: `"POST /api/login 요청을 보낸다"`
- 좋은 예: `"유효한 자격증명으로 로그인할 수 있다"`

## Playwright 설정 컨벤션

### playwright.config.ts 필수 확인 항목

| 설정 | 권장값 |
|-----|-------|
| `testDir` | `./tests` |
| `workers` | 로컬: 2~4, CI: 1 |
| `headless` | `true` (로컬/CI 동일) |
| `screenshot` | `'only-on-failure'` |
| `video` | `'retain-on-failure'` |
| `trace` | `'on-first-retry'` |
| `timeout` | 30000ms |
| `expect.timeout` | 5000ms |

### 배치 크기(Workers) 선택 가이드

- CPU 코어 수 이하로 설정
- 로컬 개발: 2-4개 권장
- CI 환경: 리소스 제한 시 1-2개
- 명령줄 오버라이드: `npx playwright test --workers=N`

## 테스트 작성 컨벤션

### Page Object Model (POM) 사용

- 재사용 가능한 페이지 객체를 `pages/` 디렉토리에 정의
- Locator를 POM 클래스에 캡슐화하고, 테스트에서는 행위 메서드만 호출

### 셀렉터 규칙

- **필수**: `data-testid` 속성 사용 (`[data-testid="submit-button"]`)
- **금지**: CSS 클래스(`button.btn-primary`), 태그 기반 셀렉터

### 대기 조건 규칙

- **금지**: `page.waitForTimeout(N)` (하드코딩된 대기)
- **필수**: 명시적 조건 대기 (`page.waitForURL()`, `expect(locator).toBeVisible()`)

### Fixtures를 사용한 테스트 데이터 관리

- `base.extend<TestFixtures>()`로 테스트 데이터를 fixture로 정의
- 테스트 간 데이터 독립성을 보장

### API Mocking을 활용한 테스트 격리

- `page.route()` + `route.fulfill()`로 외부 API 응답을 모킹
- 네트워크 의존성 없이 안정적인 테스트 실행

### 인증 상태 재사용

- `auth.setup.ts`에서 로그인 후 `context().storageState()`로 인증 상태 저장
- `playwright.config.ts`의 `projects`에서 `dependencies: ['setup']`으로 의존성 설정
- 인증이 필요한 테스트는 `storageState` 옵션으로 재사용

## 테스트 격리 규칙

- 각 테스트는 독립적으로 실행 가능해야 한다
- **금지**: 테스트 간 전역 변수/상태 공유
- **필수**: 각 테스트가 자체 데이터를 Given 단계에서 준비

## 금지 사항

1. **하드코딩된 대기 시간** - `waitForTimeout()` 대신 `waitForSelector()` 사용
2. **테스트 간 의존성** - 각 테스트는 독립 실행 가능해야 함
3. **프로덕션 환경 테스트** - 테스트 전용 환경 또는 모킹 사용
4. **불안정한 셀렉터** - CSS 클래스 대신 `data-testid` 사용
5. **기술적 테스트 설명** - 비즈니스 시나리오로 작성

## 테스트 실행 명령어

```bash
# 전체 테스트 실행 (headless)
npx playwright test

# 특정 파일 실행
npx playwright test login.spec.ts

# 특정 브라우저에서만 실행
npx playwright test --project=chromium

# Headed 모드로 실행 (브라우저 UI 보임)
npx playwright test --headed

# 병렬 실행 워커 수 지정
npx playwright test --workers=4

# 디버그 모드 (단계별 실행)
npx playwright test --debug

# 특정 테스트만 실행 (테스트 이름으로 필터링)
npx playwright test -g "로그인"

# UI 모드 (인터랙티브)
npx playwright test --ui

# HTML 리포트 생성
npx playwright show-report
```
