#!/usr/bin/env python3
"""
Automation Script Generator

QA가 성공적으로 완료되면 재사용 가능한 자동화 스크립트를 생성합니다.
- Jest 테스트 스위트
- Playwright E2E 테스트
- GitHub Actions CI/CD 설정
- Cron 스케줄링 설정
"""

import json
import sys
from pathlib import Path
from datetime import datetime

class AutomationScriptGenerator:
    def __init__(self, qa_result_file: str, output_dir: str):
        self.qa_result = self._load_qa_result(qa_result_file)
        self.output_dir = Path(output_dir)
        self.output_dir.mkdir(parents=True, exist_ok=True)

    def _load_qa_result(self, qa_result_file: str) -> dict:
        """QA 결과 파일을 로드합니다."""
        try:
            with open(qa_result_file, 'r') as f:
                return json.load(f)
        except:
            return {'exit_code': 1}

    def generate(self) -> bool:
        """자동화 스크립트를 생성합니다."""
        try:
            print("🔧 Generating Automation Scripts\n")

            # QA가 성공했는지 확인
            if not self._is_qa_successful():
                print("⚠️ QA was not successful. Skipping automation script generation.")
                return False

            # 1. GitHub Actions 워크플로우 생성
            self._generate_github_actions()

            # 2. Jest 테스트 스크립트 생성
            self._generate_jest_config()

            # 3. Playwright 설정 파일 생성
            self._generate_playwright_config()

            # 4. Cron 스케줄링 설정 생성
            self._generate_cron_config()

            # 5. package.json scripts 생성
            self._generate_package_scripts()

            # 6. README 생성
            self._generate_readme()

            print("\n✅ Automation scripts generated successfully!")
            print(f"📁 Output directory: {self.output_dir}\n")
            return True

        except Exception as e:
            print(f"❌ Error: {str(e)}\n")
            return False

    def _is_qa_successful(self) -> bool:
        """QA가 성공했는지 확인합니다."""
        exit_code = self.qa_result.get('exit_code', 1)
        return exit_code == 0

    def _generate_github_actions(self):
        """GitHub Actions 워크플로우를 생성합니다."""
        workflows_dir = self.output_dir / '.github/workflows'
        workflows_dir.mkdir(parents=True, exist_ok=True)

        workflow_content = """name: QA Automation

on:
  push:
    branches: [ main, develop ]
  pull_request:
    branches: [ main ]
  schedule:
    - cron: '0 2 * * *'  # 매일 2시에 실행

jobs:
  qa-automation:
    runs-on: ubuntu-latest

    strategy:
      matrix:
        node-version: [18.x, 20.x]

    steps:
      - uses: actions/checkout@v3

      - name: Use Node.js ${{ matrix.node-version }}
        uses: actions/setup-node@v3
        with:
          node-version: ${{ matrix.node-version }}
          cache: 'npm'

      - name: Install dependencies
        run: npm ci

      - name: Install Playwright browsers
        run: npx playwright install --with-deps chromium

      - name: Run QA Orchestrator
        run: python scripts/qa_orchestrator.py . --workers 4

      - name: Upload results
        if: always()
        uses: actions/upload-artifact@v3
        with:
          name: qa-results-${{ matrix.node-version }}
          path: qa-results/
          retention-days: 30

      - name: Comment PR with results
        if: github.event_name == 'pull_request'
        uses: actions/github-script@v6
        with:
          script: |
            const fs = require('fs');
            const summary = JSON.parse(fs.readFileSync('qa-results/summary.json', 'utf8'));
            const comment = `## QA Automation Results

            - Status: ${summary.status}
            - Success Rate: ${summary.metrics.success_rate}%
            - Details: See artifacts for full report
            `;
            github.rest.issues.createComment({
              issue_number: context.issue.number,
              owner: context.repo.owner,
              repo: context.repo.repo,
              body: comment
            });

      - name: Slack Notification
        if: failure()
        uses: slackapi/slack-github-action@v1
        with:
          payload: |
            {
              "text": "QA Automation Failed",
              "blocks": [
                {
                  "type": "section",
                  "text": {
                    "type": "mrkdwn",
                    "text": "QA automation for ${{ github.repository }} failed.\\nCheck: ${{ github.server_url }}/${{ github.repository }}/actions/runs/${{ github.run_id }}"
                  }
                }
              ]
            }
"""

        with open(workflows_dir / 'qa-automation.yml', 'w') as f:
            f.write(workflow_content)

        print("  ✅ Generated: .github/workflows/qa-automation.yml")

    def _generate_jest_config(self):
        """Jest 설정 파일을 생성합니다. QA E2E 테스트 경로는 Jest에서 제외합니다."""
        jest_config = {
            "preset": "ts-jest",
            "setupFilesAfterEnv": ["<rootDir>/setup-jest.ts"],
            "testPathIgnorePatterns": [
                "<rootDir>/node_modules",
                "<rootDir>/dist",
                "tests/qa-e2e"
            ],
            "coveragePathIgnorePatterns": [
                "/node_modules/"
            ],
            "collectCoverageFrom": [
                "src/**/*.ts",
                "!src/**/*.module.ts",
                "!src/main.ts"
            ],
            "coverageThreshold": {
                "global": {
                    "branches": 70,
                    "functions": 70,
                    "lines": 70,
                    "statements": 70
                }
            }
        }

        with open(self.output_dir / 'jest.config.json', 'w') as f:
            json.dump(jest_config, f, indent=2)

        print("  ✅ Generated: jest.config.json")

    def _generate_playwright_config(self):
        """Playwright 설정 파일을 생성합니다. QA E2E 테스트 전용 설정입니다."""
        playwright_config = """import { defineConfig, devices } from '@playwright/test';

/**
 * QA E2E 테스트 전용 Playwright 설정
 * 테스트 디렉토리는 tests/qa-e2e이며, 모든 테스트는 영상으로 녹화됩니다.
 * 영상 파일명은 {priority}-{component}-{scenario-slug}.webm 형식을 따릅니다.
 */
export default defineConfig({
  testDir: 'tests/qa-e2e',
  testMatch: ['**/*.qa-e2e.test.ts'],

  /* 테스트 실행 설정 */
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  workers: process.env.CI ? 1 : undefined,
  reporter: 'html',

  /* 영상 및 결과 출력 디렉토리 */
  outputDir: 'qa-results/recordings',

  use: {
    trace: 'on-first-retry',
    /* 모든 테스트를 항상 영상으로 녹화 */
    video: 'on',
  },

  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'] },
    },
  ],
});
"""

        with open(self.output_dir / 'playwright.config.ts', 'w') as f:
            f.write(playwright_config)

        print("  ✅ Generated: playwright.config.ts")

    def _generate_cron_config(self):
        """Cron 스케줄링 설정을 생성합니다."""
        cron_config = {
            "schedules": [
                {
                    "name": "daily-full-qa",
                    "cron": "0 2 * * *",
                    "description": "Full QA run every day at 2 AM",
                    "command": "python scripts/qa_orchestrator.py . --auto-generate-scripts"
                },
                {
                    "name": "weekly-extended-qa",
                    "cron": "0 3 * * 0",
                    "description": "Extended QA run every Sunday at 3 AM",
                    "command": "python scripts/qa_orchestrator.py . --workers 8 --auto-generate-scripts"
                },
                {
                    "name": "monthly-report",
                    "cron": "0 1 1 * *",
                    "description": "Generate monthly QA report on the 1st at 1 AM",
                    "command": "python scripts/qa_orchestrator.py . --generate-report-only"
                }
            ]
        }

        with open(self.output_dir / 'cron-schedules.json', 'w') as f:
            json.dump(cron_config, f, indent=2)

        print("  ✅ Generated: cron-schedules.json")

    def _generate_package_scripts(self):
        """package.json 스크립트 섹션을 생성합니다."""
        # npm test로는 QA E2E 테스트가 실행되지 않음
        # 일반 테스트(jest)와 QA E2E 테스트(playwright)를 분리하여 관리
        scripts = {
            "test": "jest --testPathIgnorePatterns='tests/qa-e2e'",
            "test:qa-e2e": "npx playwright test --config playwright.config.ts",
            "qa:full": "python scripts/qa_orchestrator.py . --auto-generate-scripts",
            "qa:quick": "npx playwright test --grep @smoke",
            "qa:debug": "npx playwright test --debug",
            "qa:ui": "npx playwright test --ui",
            "qa:report": "npx playwright show-report",
            "test:unit": "jest",
            "test:unit:watch": "jest --watch",
            "test:coverage": "jest --coverage",
            "ci:qa": "npm run qa:full",
            "ci:test": "npm run test:unit && npm run test:coverage"
        }

        script_content = f"""
# Add these scripts to your package.json:
# npm test로는 QA E2E 테스트가 실행되지 않음 (Jest만 실행)
# QA E2E 테스트는 npm run test:qa-e2e로 별도 실행

{json.dumps({"scripts": scripts}, indent=2)}

# Or run individually:
# npm test                     - 일반 테스트만 실행 (QA E2E 제외)
# npm run test:qa-e2e          - QA E2E 테스트만 실행 (Playwright)
# npm run qa:full              - Full QA automation
# npm run qa:quick             - Quick smoke tests
# npm run qa:debug             - Debug mode
# npm run qa:ui                - Interactive UI mode
# npm run qa:report            - View report
# npm run test:unit            - Unit tests
# npm run test:coverage        - Coverage report
"""

        with open(self.output_dir / 'package-scripts.json', 'w') as f:
            json.dump(scripts, f, indent=2)

        print("  ✅ Generated: package-scripts.json")

    def _generate_readme(self):
        """자동화 스크립트 README를 생성합니다."""
        readme_content = f"""# QA Automation Scripts

자동으로 생성된 QA 자동화 스크립트 모음입니다.

## 구성

- `.github/workflows/qa-automation.yml` - GitHub Actions 워크플로우
- `jest.config.json` - Jest 테스트 설정
- `playwright.config.ts` - Playwright E2E 테스트 설정
- `cron-schedules.json` - Cron 스케줄링 설정
- `package-scripts.json` - npm 스크립트

## 사전 설치

### Playwright devDependencies 설치

```bash
npm install -D @playwright/test
npx playwright install --with-deps chromium
```

## 테스트 분리 구조

`npm test`와 `npm run test:qa-e2e`는 별도로 실행됩니다.

- `npm test` - Jest 기반 일반 테스트만 실행 (QA E2E 테스트 제외)
- `npm run test:qa-e2e` - Playwright 기반 QA E2E 테스트만 실행

Jest 설정에서 `tests/qa-e2e` 경로를 `testPathIgnorePatterns`에 추가하여
일반 테스트 실행 시 QA E2E 테스트가 포함되지 않도록 분리되어 있습니다.

## 빠른 시작

### 1. npm 스크립트 추가

`package.json`의 `scripts` 섹션에 다음을 추가하세요:

```json
{json.dumps({"scripts": self._get_scripts()}, indent=2)}
```

### 2. 첫 실행

```bash
npm run qa:full
```

### 3. 결과 확인

QA 결과는 `qa-results/` 디렉토리에 저장됩니다.

## 사용 가능한 명령어

| 명령어 | 설명 |
|--------|------|
| `npm test` | 일반 테스트만 실행 (QA E2E 제외) |
| `npm run test:qa-e2e` | QA E2E 테스트만 실행 (Playwright) |
| `npm run qa:full` | 전체 QA 자동화 실행 |
| `npm run qa:quick` | 빠른 smoke 테스트만 실행 |
| `npm run qa:debug` | 디버그 모드로 테스트 실행 |
| `npm run qa:ui` | 인터랙티브 UI 모드 |
| `npm run test:unit` | 유닛 테스트 실행 |
| `npm run test:coverage` | 커버리지 리포트 생성 |

## CI/CD 통합

### GitHub Actions

`.github/workflows/qa-automation.yml`이 자동으로 생성되었습니다.

**Triggers:**
- Push to main/develop
- Pull Requests
- 매일 2시 (스케줄)

### GitLab CI

`.gitlab-ci.yml` 추가 (선택사항):

```yaml
qa:automation:
  stage: test
  script:
    - npm install
    - python scripts/qa_orchestrator.py . --auto-generate-scripts
  artifacts:
    paths:
      - qa-results/
    expire_in: 30 days
```

## Cron 스케줄링

`cron-schedules.json`을 참고하여 다음과 같이 스케줄링할 수 있습니다:

```bash
# 일일 QA 실행
0 2 * * * cd /path/to/project && python scripts/qa_orchestrator.py . --auto-generate-scripts
```

## 환경 변수

```bash
# Slack 알림 활성화
export SLACK_WEBHOOK_URL=https://hooks.slack.com/...

# 병렬 워커 수 조정
export QA_WORKERS=8

# 비디오 녹화 활성화/비활성화
export RECORD_VIDEO=true
```

## 문제 해결

### 테스트가 실패하는 경우

1. 로그 확인: `qa-results/logs/`
2. 동영상 확인: `qa-results/recordings/`
3. Trace 파일 확인: `npx playwright show-trace qa-results/trace.zip`

### 자동화 스크립트가 생성되지 않는 경우

- QA 성공률이 80% 이상인지 확인
- `qa-results/summary.json`에서 상세 정보 확인

## 피드백 및 개선

이 자동화 스크립트는 QA Automation Orchestrator에서 생성되었습니다.
개선 사항이나 버그는 GitHub Issues로 보고해주세요.

---

Generated on: {datetime.now().isoformat()}
"""

        with open(self.output_dir / 'AUTOMATION-README.md', 'w') as f:
            f.write(readme_content)

        print("  ✅ Generated: AUTOMATION-README.md")

    def _get_scripts(self) -> dict:
        """npm 스크립트를 반환합니다."""
        return {
            "test": "jest --testPathIgnorePatterns='tests/qa-e2e'",
            "test:qa-e2e": "npx playwright test --config playwright.config.ts",
            "qa:full": "python scripts/qa_orchestrator.py . --auto-generate-scripts",
            "qa:quick": "npx playwright test --grep @smoke",
            "qa:debug": "npx playwright test --debug",
            "qa:ui": "npx playwright test --ui",
            "test:unit": "jest",
            "test:coverage": "jest --coverage"
        }

def main():
    if len(sys.argv) < 2:
        print("Usage: automation_script_generator.py --qa-result <result-file> --output-dir <output-dir>")
        sys.exit(1)

    qa_result_file = None
    output_dir = "./automation-scripts"

    if "--qa-result" in sys.argv:
        idx = sys.argv.index("--qa-result")
        if idx + 1 < len(sys.argv):
            qa_result_file = sys.argv[idx + 1]

    if "--output-dir" in sys.argv:
        idx = sys.argv.index("--output-dir")
        if idx + 1 < len(sys.argv):
            output_dir = sys.argv[idx + 1]

    if not qa_result_file:
        print("Error: --qa-result argument is required")
        sys.exit(1)

    generator = AutomationScriptGenerator(qa_result_file, output_dir)
    success = generator.generate()

    sys.exit(0 if success else 1)

if __name__ == "__main__":
    main()
