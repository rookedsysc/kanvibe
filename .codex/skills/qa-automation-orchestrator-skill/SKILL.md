---
name: qa-automation-orchestrator-skill
description: End-to-end QA automation orchestration that auto-discovers test scope, generates test cases, executes tests, records results as video, and creates reusable automation scripts. Use when you need to: (1) Automatically determine QA scope for a project, (2) Generate test cases from components, (3) Execute E2E tests with automatic video recording, (4) Analyze QA results and create reports, (5) Generate CI/CD automation scripts for future runs.
---

# QA Automation Orchestrator

End-to-end orchestration skill that combines Playwright testing, video recording, and automation script generation into a unified QA workflow. Automatically analyzes project scope, generates tests, executes them with video recording, and creates reusable automation scripts.

---

## 사전 설정

Playwright 설치, `playwright.config.ts` 생성, `npm test` 분리 설정은 **[qa_e2e_setup_guide.md](references/qa_e2e_setup_guide.md)**를 참고합니다.

핵심 요약:
- `npm install -D @playwright/test` + `npx playwright install chromium`
- `npm test`: 일반 테스트만 실행 / `npm run test:qa-e2e`: QA E2E 테스트만 실행

---

## 최초 세팅 워크플로우

QA E2E 테스트를 처음 실행할 때, 다음 항목의 존재 여부를 자동으로 확인합니다.

### 자동 체크 대상

- `tests/qa-e2e/` 디렉토리 존재 여부
- `playwright.config.ts` 파일 존재 여부

### 세팅 미완료 시 동작

위 항목 중 하나라도 없으면, `references/qa_e2e_setup_guide.md` 기반의 초기 세팅 가이드를 자동으로 출력합니다. 가이드에는 디렉토리 생성, Playwright 설치, 설정 파일 생성, npm scripts 등록까지의 전체 절차가 포함됩니다.

```bash
# 세팅 상태 확인 예시
if [ ! -d "tests/qa-e2e" ] || [ ! -f "playwright.config.ts" ]; then
  echo "QA E2E 초기 세팅이 필요합니다. references/qa_e2e_setup_guide.md를 참고하세요."
fi
```

---

## Core Capabilities

### 1. QA Scope Analysis
자동으로 프로젝트 구조를 분석하고 테스트할 컴포넌트와 범위를 확정합니다.

```bash
python scripts/qa_scope_analyzer.py <project-path> [--config-file]
```

**Features:**
- 컴포넌트 자동 검출 (React, Vue, Angular 등)
- 테스트 범위 산정 (Critical, High, Medium, Low)
- 의존성 분석 및 테스트 순서 결정
- 기존 테스트 커버리지 분석

### 2. Automatic Test Case Generation
확정된 범위에서 자동으로 테스트 케이스를 생성합니다.

```bash
python scripts/test_case_generator.py <project-path> --scope-result <scope-file>
```

**Features:**
- Given-When-Then 패턴 적용
- 컴포넌트별 테스트 케이스 자동 생성
- Integration test scenarios 자동 도출
- Playwright E2E 테스트 코드 생성 (`tests/qa-e2e/*.qa-e2e.test.ts`)

### 3. QA Orchestration & Execution
테스트를 실행하고 자동으로 동영상을 촬영합니다.

```bash
python scripts/qa_orchestrator.py \
  --scope-file <scope.json> \
  --tests-dir ./tests/qa-e2e \
  --record-video \
  --output-dir ./qa-results
```

**Features:**
- QA 스코프 확정
- Playwright 테스트 자동 실행
- 실시간 동영상 녹화 (playwright-recording 통합)
- 테스트 진행 상황 모니터링
- 자동 결과 분석 및 리포트 생성

### 4. Result Analysis & Video Generation
QA 결과를 분석하고 정리된 동영상 리포트를 생성합니다.

**Output:**
```
qa-results/
├── summary.json (테스트 통과율, 실패 항목)
├── detailed-report.html (시각적 리포트)
├── recordings/ (각 테스트의 동영상)
│   ├── critical-login-valid-credentials.webm
│   ├── high-checkout-payment-flow.webm
│   ├── medium-profile-edit-avatar.webm
│   └── ...
└── logs/ (테스트 로그)
```

### 5. Automation Script Generation (Success Only)
QA가 성공적으로 완료되면 재사용 가능한 자동화 스크립트를 자동으로 생성합니다.

```bash
python scripts/automation_script_generator.py \
  --qa-result ./qa-results/summary.json \
  --output-dir ./automation-scripts
```

**Generated Outputs:**
- `playwright-tests.ts` - Playwright E2E 테스트 (`tests/qa-e2e/` 기준)
- `.github/workflows/qa-automation.yaml` - GitHub Actions CI/CD
- `cron-schedule.js` - 정기적 실행 설정

---

## 테스트 파일 규칙

### 파일 경로 및 네이밍

모든 QA E2E 테스트 파일은 `tests/qa-e2e/` 디렉토리에 위치하며, 다음 규칙을 따릅니다.

| 항목 | 규칙 |
|------|------|
| **디렉토리** | `tests/qa-e2e/` |
| **파일명 형식** | `{component-name}.qa-e2e.test.ts` |
| **케이스** | kebab-case |

예시:
```
tests/qa-e2e/
├── login.qa-e2e.test.ts
├── checkout.qa-e2e.test.ts
├── user-profile.qa-e2e.test.ts
└── product-search.qa-e2e.test.ts
```

### 동영상 네이밍 규칙

녹화된 동영상 파일은 `{priority}-{component}-{scenario-slug}.webm` 형식을 따릅니다.

| 요소 | 설명 | 예시 |
|------|------|------|
| **priority** | 테스트 우선순위 (critical, high, medium, low) | `critical` |
| **component** | 테스트 대상 컴포넌트명 | `login` |
| **scenario-slug** | 시나리오 요약 (kebab-case) | `valid-credentials` |

예시:
```
recordings/
├── critical-login-valid-credentials.webm
├── critical-login-invalid-password.webm
├── high-checkout-payment-flow.webm
├── high-checkout-coupon-apply.webm
├── medium-profile-edit-avatar.webm
└── low-settings-theme-toggle.webm
```

---

## Workflow

### Simple One-Command Execution

```bash
python scripts/qa_orchestrator.py \
  --project-path ./src \
  --record-video \
  --auto-generate-scripts
```

이 명령 하나로:
1. QA 범위 자동 확정
2. 테스트 케이스 자동 생성
3. 테스트 실행
4. 동영상 녹화
5. 결과 분석
6. 자동화 스크립트 생성

### Step-by-Step Execution

더 세밀한 제어가 필요한 경우:

```bash
# Step 1: Analyze scope
python scripts/qa_scope_analyzer.py ./src --output scope-result.json

# Step 2: Review and edit scope (optional)
# scope-result.json을 열어 테스트 우선순위 조정

# Step 3: Generate test cases
python scripts/test_case_generator.py ./src --scope-result scope-result.json

# Step 4: Run orchestrator with video recording
python scripts/qa_orchestrator.py \
  --scope-file scope-result.json \
  --tests-dir ./tests/qa-e2e \
  --record-video \
  --output-dir ./qa-results

# Step 5: Generate automation scripts if successful
if [ $? -eq 0 ]; then
  python scripts/automation_script_generator.py \
    --qa-result ./qa-results/summary.json \
    --output-dir ./automation-scripts
fi
```

---

## Configuration

### qa.config.json

프로젝트 루트에 `qa.config.json` 생성 (선택사항):

```json
{
  "projectPath": "./src",
  "testFramework": "playwright",
  "testDir": "./tests/qa-e2e",
  "testMatch": "**/*.qa-e2e.test.ts",
  "recordVideo": true,
  "videoFormat": "webm",
  "videoNaming": "{priority}-{component}-{scenario-slug}",
  "parallelWorkers": 4,
  "criticalComponentsOnly": false,
  "autoGenerateScripts": true,
  "ciProvider": "github-actions",
  "slackNotification": {
    "enabled": false,
    "webhookUrl": "https://hooks.slack.com/..."
  }
}
```

---

## Output Structure

```
qa-results/
├── summary.json                                  # 전체 QA 결과 (통과율, 실패 항목)
├── detailed-report.html                          # 대시보드 형식 리포트
├── scope-analysis.json                           # 확정된 QA 범위
├── test-plan.json                                # 생성된 테스트 계획
├── recordings/                                   # 동영상 기록
│   ├── critical-login-valid-credentials.webm     # 로그인 정상 인증 테스트
│   ├── critical-login-invalid-password.webm      # 로그인 비밀번호 오류 테스트
│   ├── high-checkout-payment-flow.webm           # 결제 플로우 테스트
│   ├── high-checkout-coupon-apply.webm           # 쿠폰 적용 테스트
│   ├── medium-profile-edit-avatar.webm           # 프로필 아바타 수정 테스트
│   └── low-settings-theme-toggle.webm            # 테마 전환 테스트
├── logs/
│   ├── scope-analysis.log
│   ├── test-generation.log
│   ├── test-execution.log
│   └── video-generation.log
└── failed-tests/                                 # 실패한 테스트만 분리
    ├── screenshot-critical-login-invalid-password.png
    ├── critical-login-invalid-password.webm
    └── error-trace-critical-login-invalid-password.txt
```

---

## Integration with Other Skills

### playwright-recording
자동으로 각 테스트 실행 중 동영상을 녹화합니다. 커서 시각화와 클릭 효과가 자동 추가됩니다.

### qa-testing-playwright
생성된 테스트 케이스를 Playwright로 실행합니다. 안정성과 병렬 실행을 자동으로 관리합니다.

### senior-qa
QA 결과 분석, 커버리지 계산, 품질 메트릭 생성에 사용됩니다.

### test-writer-skill
Given-When-Then 패턴으로 테스트 코드를 생성할 때 사용됩니다.

---

## Key Features

| Feature | Benefit |
|---------|---------|
| **Auto Scope Detection** | 수동 범위 설정 불필요 |
| **Video Recording** | QA 과정 시각화 및 문서화 |
| **Automation Script Generation** | 향후 CI/CD 자동화 가능 |
| **Parallel Execution** | 빠른 테스트 완료 |
| **Detailed Reports** | 시각적 리포트 생성 |
| **Integration Ready** | GitHub Actions, GitLab CI 등 |
| **Test Isolation** | 일반 테스트와 QA E2E 테스트 완전 분리 |
| **Priority-based Naming** | 동영상 파일명에서 우선순위 즉시 파악 |

---

## References

자세한 내용은 다음 문서를 참고하세요:

- **[component_analysis_patterns.md](references/component_analysis_patterns.md)** - 컴포넌트 자동 분석 방법
- **[test_case_estimation.md](references/test_case_estimation.md)** - 테스트 케이스 산정 방법론
- **[qa_workflow_guide.md](references/qa_workflow_guide.md)** - 전체 워크플로우 상세 가이드
- **[automation_best_practices.md](references/automation_best_practices.md)** - 자동화 스크립트 모범 사례
- **[qa_e2e_setup_guide.md](references/qa_e2e_setup_guide.md)** - QA E2E 초기 세팅 가이드

---

## Common Commands

```bash
# 전체 QA 오토메이션 한 번에 실행
qa_orchestrator --auto

# 특정 컴포넌트만 테스트
qa_orchestrator --components auth,checkout

# 진행 중인 QA 재개
qa_orchestrator --resume

# 자동화 스크립트만 생성 (QA 결과 있을 때)
automation_script_generator --from-previous-result

# 리포트만 재생성
qa_orchestrator --regenerate-report

# QA E2E 테스트만 실행
npm run test:qa-e2e

# 일반 테스트만 실행
npm test
```

---

## Troubleshooting

### 동영상이 생성되지 않음
- Playwright 설치 확인: `npx playwright install chromium`
- 디렉토리 쓰기 권한 확인
- `references/qa_workflow_guide.md` 참고

### 테스트 케이스 생성 실패
- 프로젝트 구조 확인 (src/, components/ 등)
- `--verbose` 플래그로 디버그 로그 확인

### 자동화 스크립트 생성 안 됨
- QA 성공 여부 확인 (success rate > 80%)
- CI 제공자 설정 확인

### QA E2E 테스트가 일반 테스트에 포함됨
- `jest.config.ts`의 `testPathIgnorePatterns`에 `/tests/qa-e2e/` 포함 여부 확인
- `playwright.config.ts`의 `testDir`이 `./tests/qa-e2e`로 설정되어 있는지 확인

### 초기 세팅 가이드가 출력되지 않음
- `tests/qa-e2e/` 디렉토리와 `playwright.config.ts` 파일이 이미 존재하는지 확인
- 가이드는 두 항목 중 하나라도 없을 때만 출력됨

---

## Next Steps

1. `.claude/skills/qa-automation-orchestrator-skill/scripts/` 의 스크립트 검토
2. `qa.config.json` 프로젝트에 맞게 커스터마이징
3. `python scripts/qa_orchestrator.py --help` 로 옵션 확인
4. 첫 실행: `qa_orchestrator --auto`

---
