# QA E2E 테스트 최초 세팅 가이드

QA E2E 테스트가 전혀 구성되지 않은 프로젝트에서 QA 오케스트레이터를 사용하기 위한 단계별 세팅 가이드다.

## 사전 요구사항

- **Node.js 18 이상** (Playwright가 Node.js 18+을 요구함)
- **npm** 또는 **yarn** 패키지 매니저

버전 확인:

```bash
node --version  # v18.x.x 이상이어야 함
npm --version
```

## Playwright 설치

```bash
npm install -D @playwright/test
npx playwright install chromium
```

`devDependencies`(`-D` 플래그)로 설치하는 이유는 Playwright가 프로덕션 런타임에서는 필요하지 않은 테스트 전용 도구이기 때문이다. 프로덕션 빌드 크기를 불필요하게 늘리지 않기 위해 개발 의존성으로 분리한다.

`npx playwright install chromium`은 Chromium 브라우저 바이너리만 별도로 설치하는 명령이다. QA E2E 테스트는 기본적으로 Chromium 단일 브라우저로 실행하므로, 불필요한 Firefox/WebKit 설치를 생략하여 디스크 공간과 설치 시간을 절약한다.

## 디렉토리 구조 생성

```bash
mkdir -p tests/qa-e2e
```

`tests/qa-e2e` 경로를 사용하는 이유:

- **`src/`와 분리**: 테스트 코드가 소스 코드와 혼재되지 않아 빌드 대상에서 자연스럽게 제외된다
- **`npm test`에서 제외 용이**: Jest나 Vitest의 기본 테스트 디렉토리(`__tests__/`, `src/`)와 분리되어 있어, 일반 단위/통합 테스트 실행 시 QA E2E 테스트가 포함되지 않도록 설정하기 쉽다
- **`qa-e2e` 접두사**: 일반 E2E 테스트와 QA 전용 E2E 테스트를 구분하여, QA 오케스트레이터가 관리하는 테스트임을 명확히 한다

## playwright.config.ts 생성

프로젝트 루트에 `playwright.config.ts` 파일을 생성한다:

```typescript
import { defineConfig, devices } from '@playwright/test';

export default defineConfig({
  /** QA E2E 테스트 디렉토리 */
  testDir: 'tests/qa-e2e',

  /** .qa-e2e.test.ts 패턴의 파일만 테스트 대상으로 인식 */
  testMatch: ['**/*.qa-e2e.test.ts'],

  /** 테스트 실행 동영상을 항상 녹화 */
  use: {
    video: 'on',
  },

  /** 테스트 결과물(동영상, 트레이스 등) 저장 경로 */
  outputDir: 'qa-results',

  /** HTML 리포트 생성 설정 */
  reporter: [['html', { outputFolder: 'qa-results/report' }]],

  /** 브라우저 프로젝트 설정: Chromium 기본 */
  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'] },
    },
  ],
});
```

각 설정 항목 설명:

- **`testDir`**: Playwright가 테스트 파일을 탐색할 루트 디렉토리
- **`testMatch`**: `*.qa-e2e.test.ts` 패턴에 해당하는 파일만 테스트로 인식하여, 디렉토리 내 다른 유틸리티 파일이나 헬퍼 파일이 테스트로 실행되지 않도록 한다
- **`use.video: 'on'`**: 모든 테스트 실행 시 동영상을 녹화한다. QA 오케스트레이터가 테스트 결과를 시각적으로 검증하는 데 활용한다
- **`outputDir`**: 녹화된 동영상, 스크린샷, 트레이스 파일 등의 저장 경로
- **`reporter`**: HTML 형태의 테스트 리포트를 `qa-results/report/`에 생성한다
- **`projects`**: Chromium 단일 브라우저로 테스트를 실행한다. 필요 시 Firefox, WebKit을 추가할 수 있다

## npm test에서 QA 테스트 제외 설정

QA E2E 테스트는 일반 `npm test` 명령으로 실행되어서는 안 된다. 사용 중인 테스트 프레임워크에 따라 아래 설정을 적용한다.

### Jest 사용 시

`jest.config.ts` 또는 `jest.config.js`에 다음을 추가한다:

```typescript
export default {
  // 기존 설정 유지
  testPathIgnorePatterns: [
    '/node_modules/',
    'tests/qa-e2e',
  ],
};
```

### Vitest 사용 시

`vitest.config.ts`에 다음을 추가한다:

```typescript
import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    exclude: [
      'node_modules/**',
      'tests/qa-e2e/**',
    ],
  },
});
```

### package.json scripts 설정

`package.json`의 `scripts` 섹션에 다음을 추가한다:

```json
{
  "scripts": {
    "test": "jest --testPathIgnorePatterns='tests/qa-e2e'",
    "test:qa-e2e": "npx playwright test --config playwright.config.ts"
  }
}
```

- **`test`**: 일반 단위/통합 테스트만 실행한다. QA E2E 테스트 경로를 명시적으로 제외한다
- **`test:qa-e2e`**: QA E2E 테스트만 실행한다. `playwright.config.ts`를 명시적으로 지정하여 설정 파일을 정확히 참조한다

Vitest를 사용하는 프로젝트의 경우, `test` 스크립트를 환경에 맞게 조정한다:

```json
{
  "scripts": {
    "test": "vitest run",
    "test:qa-e2e": "npx playwright test --config playwright.config.ts"
  }
}
```

## 테스트 파일 네이밍 규칙

### 형식

```
{kebab-case-component}.qa-e2e.test.ts
```

### 규칙

- 컴포넌트 또는 페이지 이름을 **kebab-case**로 작성한다
- `.qa-e2e.test.ts` 접미사를 반드시 사용한다
- `playwright.config.ts`의 `testMatch` 패턴과 일치해야 테스트로 인식된다

### 예시

| 파일명 | 대상 |
|--------|------|
| `login-form.qa-e2e.test.ts` | 로그인 폼 |
| `checkout-page.qa-e2e.test.ts` | 결제 페이지 |
| `user-profile-settings.qa-e2e.test.ts` | 사용자 프로필 설정 |
| `product-search-filter.qa-e2e.test.ts` | 상품 검색 필터 |

## 동영상 파일 네이밍 규칙

### 형식

```
{priority}-{component}-{scenario-slug}.webm
```

### priority 값

| 우선순위 | 설명 |
|---------|------|
| `critical` | 핵심 비즈니스 플로우 (로그인, 결제 등) |
| `high` | 주요 기능 (검색, 필터, 데이터 입력 등) |
| `medium` | 보조 기능 (설정 변경, 프로필 수정 등) |
| `low` | 부가 기능 (UI 애니메이션, 툴팁 등) |

### 예시

| 파일명 | 설명 |
|--------|------|
| `critical-login-valid-credentials.webm` | 올바른 자격증명으로 로그인 |
| `critical-checkout-payment-success.webm` | 결제 성공 플로우 |
| `high-search-keyword-filter.webm` | 키워드 검색 필터 동작 |
| `medium-profile-avatar-upload.webm` | 프로필 아바타 업로드 |
| `low-tooltip-hover-display.webm` | 툴팁 호버 표시 |

## .gitignore 추가

프로젝트 루트의 `.gitignore` 파일에 다음 항목을 추가한다:

```gitignore
# QA E2E 테스트 결과물
qa-results/
test-results/
playwright-report/
```

테스트 결과물(동영상, 스크린샷, HTML 리포트)은 실행할 때마다 새로 생성되므로, 버전 관리 대상에서 제외한다. CI/CD 환경에서는 별도의 아티팩트 저장소에 업로드하는 방식을 권장한다.

## 세팅 완료 체크리스트

모든 세팅이 올바르게 완료되었는지 아래 항목을 확인한다:

- [ ] `@playwright/test`가 `package.json`의 `devDependencies`에 있음
- [ ] `tests/qa-e2e/` 디렉토리가 프로젝트 루트에 존재함
- [ ] `playwright.config.ts`가 프로젝트 루트에 있음
- [ ] `npm test` 실행 시 QA E2E 테스트가 실행되지 않음
- [ ] `npm run test:qa-e2e` 실행 시 QA E2E 테스트만 실행됨
