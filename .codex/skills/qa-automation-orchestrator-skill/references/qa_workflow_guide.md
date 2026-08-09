# QA Workflow 상세 가이드

이 문서는 qa-automation-orchestrator-skill의 전체 워크플로우를 상세히 설명합니다.

## 사전 요구사항

테스트 실행 전에 Playwright 환경이 설정되어 있어야 합니다. 초기 세팅이 완료되지 않은 경우 **[qa_e2e_setup_guide.md](qa_e2e_setup_guide.md)**를 참고하세요.

## 1단계: QA 범위 분석 (qa_scope_analyzer.py)

### 목표
프로젝트 구조를 분석하여 테스트할 컴포넌트와 범위를 자동으로 확정합니다.

### 실행
```bash
python scripts/qa_scope_analyzer.py ./src --output qa-scope.json
```

### 분석 대상
- React/Vue/Angular 컴포넌트
- 페이지 및 라우트
- 서비스 및 유틸리티
- API 엔드포인트

### 우선순위 산정 규칙
- **CRITICAL**: 인증, 결제, 거래 관련
- **HIGH**: 폼, 모달, 네비게이션
- **MEDIUM**: 일반 컴포넌트
- **LOW**: 유틸리티, 헬퍼

### 출력 (qa-scope.json)
```json
{
  "total_components": 42,
  "components": [
    {
      "name": "LoginComponent",
      "type": "component",
      "priority": "CRITICAL",
      "path": "src/components/LoginComponent.tsx"
    }
  ],
  "by_priority": {
    "CRITICAL": ["LoginComponent", "CheckoutComponent"],
    "HIGH": ["FormComponent", "ModalComponent"]
  }
}
```

## 2단계: 테스트 케이스 생성 (test_case_generator.py)

### 목표
확정된 QA 범위에서 자동으로 테스트 케이스를 생성합니다.

### 실행
```bash
python scripts/test_case_generator.py ./src --scope-file qa-scope.json
```

### 생성되는 테스트 케이스 유형

#### 컴포넌트 테스트
- 렌더링 테스트
- 사용자 상호작용 테스트
- 에러 상태 테스트

#### 페이지 테스트
- 페이지 로딩 테스트
- 네비게이션 테스트
- 반응형 디자인 테스트

#### 서비스 테스트
- 데이터 반환 테스트
- 에러 처리 테스트
- 유효성 검증 테스트

### 출력 (qa-test-plan.json)

테스트 파일은 `tests/qa-e2e/` 디렉토리에 `*.qa-e2e.test.ts` 형식(kebab-case)으로 생성됩니다.

```json
{
  "total_test_cases": 127,
  "by_type": {
    "unit": 45,
    "integration": 32,
    "e2e": 50
  },
  "test_cases": {
    "e2e": [
      {
        "id": "LoginComponent-1",
        "title": "LoginComponent renders correctly",
        "priority": "CRITICAL",
        "scenario": {
          "Given": "User is on login page",
          "When": "Page loads",
          "Then": "Login form is visible"
        },
        "code": "// tests/qa-e2e/login-renders.qa-e2e.test.ts\ntest('LoginComponent renders correctly', ...)"
      },
      {
        "id": "LoginComponent-2",
        "title": "LoginComponent valid credentials",
        "priority": "CRITICAL",
        "scenario": {
          "Given": "User is on login page",
          "When": "User enters valid credentials",
          "Then": "User is redirected to dashboard"
        },
        "code": "// tests/qa-e2e/login-valid-credentials.qa-e2e.test.ts\ntest('LoginComponent valid credentials', ...)"
      }
    ]
  }
}
```

## 3단계: 테스트 실행 (qa_orchestrator.py)

### 목표
생성된 테스트를 Playwright로 실행하고 동영상을 녹화합니다.

### 실행
```bash
python scripts/qa_orchestrator.py . --record-video --output-dir ./qa-results
```

### 실행 과정
1. Playwright 테스트 시작
2. 각 테스트별 동영상 녹화 (playwright-recording 사용)
3. 테스트 실패 시 스크린샷 캡처
4. Trace 파일 저장

### 병렬 실행
```bash
python scripts/qa_orchestrator.py . --workers 8 --record-video
```

### 동영상 네이밍

동영상 파일명은 `{priority}-{component}-{scenario-slug}.webm` 형식을 따릅니다. 상세 규칙은 [SKILL.md](../SKILL.md)의 "동영상 네이밍 규칙" 섹션을 참고하세요.

### 출력 구조
```
qa-results/
├── test-results.json
├── summary.json
├── recordings/
│   ├── critical-login-valid-credentials.webm
│   ├── critical-checkout-payment-flow.webm
│   └── high-navigation-full-flow.webm
├── screenshots/
│   ├── failed-test-1.png
│   └── failed-test-2.png
└── logs/
    ├── test-execution.log
    └── playwright-trace.zip
```

## 4단계: 결과 분석

### 성공 기준
- 테스트 성공률 > 80%
- Critical 테스트 100% 통과
- 동영상 생성 완료

### 메트릭
```json
{
  "success_rate": 95,
  "passed": 121,
  "failed": 6,
  "duration": "5m 32s",
  "by_priority": {
    "CRITICAL": {
      "total": 12,
      "passed": 12,
      "failed": 0
    },
    "HIGH": {
      "total": 35,
      "passed": 34,
      "failed": 1
    }
  }
}
```

## 5단계: 자동화 스크립트 생성

### 조건
- QA 성공률 >= 80%

### 생성 파일
- `.github/workflows/qa-automation.yml` - GitHub Actions 워크플로우
- `playwright.config.ts` - Playwright 설정 (QA E2E 테스트 전용)
- `jest.config.json` - Jest 설정 (`testPathIgnorePatterns`에 `tests/qa-e2e`를 추가하여 QA E2E 테스트를 Jest 실행에서 제외)
- `package-scripts.json` - npm 스크립트
- `cron-schedules.json` - Cron 스케줄

### 자동 CI/CD 설정
```yaml
name: QA Automation
on:
  push:
    branches: [main]
  schedule:
    - cron: '0 2 * * *'
```

## 트러블슈팅

### 테스트 실패
1. `qa-results/logs/`에서 로그 확인
2. `qa-results/recordings/`에서 동영상 확인
3. 실패한 테스트의 스크린샷 검토

### 동영상이 생성되지 않음
```bash
# Playwright 설치 확인
npx playwright install chromium

# 권한 확인
chmod -R 755 qa-results/
```

### 메모리 부족
```bash
# 병렬 워커 수 감소
python scripts/qa_orchestrator.py . --workers 2
```

## 성능 최적화

### 병렬 실행
```bash
# 8개 워커로 실행
python scripts/qa_orchestrator.py . --workers 8
```

### 선택적 테스트 실행
```bash
# CRITICAL만 테스트
npx playwright test --grep @critical
```

### 메모리 최적화
- 동영상 녹화 비활성화: `--no-record-video`
- 스크린샷 비활성화: `--no-screenshots`

## 다음 단계

1. **GitHub Actions 설정**: `.github/workflows/qa-automation.yml` 커밋
2. **npm 스크립트 추가**: `package.json`에 스크립트 추가
3. **Cron 스케줄링**: 매일 2시에 자동 실행
4. **Slack 알림**: 실패 시 Slack 알림 설정
