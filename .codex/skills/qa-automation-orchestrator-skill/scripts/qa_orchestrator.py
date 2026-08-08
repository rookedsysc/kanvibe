#!/usr/bin/env python3
"""
QA Orchestrator (Main Script)

QA 전체 워크플로우를 조율합니다:
1. QA 범위 확정
2. 테스트 케이스 생성
3. Playwright 테스트 실행
4. 동영상 녹화
5. 결과 분석
6. 자동화 스크립트 생성 (성공 시)
"""

import subprocess
import json
import sys
from pathlib import Path
from datetime import datetime
from typing import Dict, Any

class QAOrchestrator:
    def __init__(self, config: Dict[str, Any]):
        self.config = config
        self.project_path = Path(config.get('project_path', '.'))
        self.output_dir = Path(config.get('output_dir', './qa-results'))
        self.output_dir.mkdir(parents=True, exist_ok=True)
        self.results = {
            'timestamp': datetime.now().isoformat(),
            'steps': {},
            'success': False,
            'metrics': {}
        }

    def _check_initial_setup(self) -> bool:
        """Playwright E2E 테스트 환경의 초기 세팅 여부를 확인합니다.

        @returns 테스트 디렉토리, playwright 설정 파일, devDependencies 등록이 모두 갖추어졌으면 True
        """
        e2e_dir = self.project_path / 'tests' / 'qa-e2e'
        playwright_config = self.project_path / 'playwright.config.ts'
        package_json = self.project_path / 'package.json'

        if not e2e_dir.exists():
            print("  ⚠️ tests/qa-e2e 디렉토리가 존재하지 않습니다.")
            return False

        if not playwright_config.exists():
            print("  ⚠️ playwright.config.ts 파일이 존재하지 않습니다.")
            return False

        if not package_json.exists():
            print("  ⚠️ package.json 파일이 존재하지 않습니다.")
            return False

        try:
            with open(package_json, 'r') as f:
                pkg = json.load(f)
            dev_deps = pkg.get('devDependencies', {})
            if '@playwright/test' not in dev_deps:
                print("  ⚠️ @playwright/test가 devDependencies에 등록되어 있지 않습니다.")
                return False
        except (json.JSONDecodeError, IOError) as e:
            print(f"  ❌ package.json 파싱 실패: {str(e)}")
            return False

        return True

    def _run_initial_setup(self):
        """Playwright E2E 테스트 환경의 초기 세팅 가이드를 출력합니다."""
        print("\n📦 E2E 테스트 환경 초기 세팅이 필요합니다.")
        print("  아래 단계를 순서대로 진행해주세요:\n")
        print("  1. Playwright 패키지 설치:")
        print("     npm install -D @playwright/test\n")
        print("  2. Chromium 브라우저 설치:")
        print("     npx playwright install chromium\n")
        print("  3. E2E 테스트 디렉토리 생성:")
        print("     mkdir -p tests/qa-e2e\n")
        print("  4. playwright.config.ts 생성 (testDir: 'tests/qa-e2e', video 설정 포함):")
        print("     - testDir을 'tests/qa-e2e'로 지정")
        print("     - use.video를 'on' 또는 'retain-on-failure'로 설정\n")
        print("  5. jest/vitest 설정에서 testPathIgnorePatterns에 'tests/qa-e2e' 추가\n")
        print("  📖 상세 가이드: references/qa_e2e_setup_guide.md 를 참조하세요.\n")

    def run(self) -> bool:
        """전체 QA 워크플로우를 실행합니다."""
        try:
            print("🚀 Starting QA Automation Orchestrator\n")

            # 초기 세팅 확인 후 미비 시 가이드 출력 후 종료
            if not self._check_initial_setup():
                self._run_initial_setup()
                return False

            # Step 1: QA 범위 확정
            if not self._run_scope_analysis():
                return False

            # Step 2: 테스트 케이스 생성
            if not self._generate_test_cases():
                return False

            # Step 3: 테스트 실행 (동영상 녹화 포함)
            if not self._run_tests():
                return False

            # Step 4: 결과 분석
            if not self._analyze_results():
                return False

            # Step 5: 자동화 스크립트 생성 (성공 시)
            if self.results['success']:
                self._generate_automation_scripts()

            # 최종 리포트 생성
            self._generate_final_report()

            print(f"\n✅ QA Automation Complete!")
            return True

        except Exception as e:
            print(f"❌ Error: {str(e)}")
            return False

    def _run_scope_analysis(self) -> bool:
        """Step 1: QA 범위를 분석합니다."""
        print("📍 Step 1: Analyzing QA Scope")

        try:
            result = subprocess.run([
                'python', 'scripts/qa_scope_analyzer.py',
                str(self.project_path),
                '--output', str(self.output_dir / 'qa-scope.json')
            ], capture_output=True, text=True, timeout=60)

            if result.returncode == 0:
                self.results['steps']['scope_analysis'] = 'success'
                print("  ✅ Scope analysis complete\n")
                return True
            else:
                print(f"  ❌ Scope analysis failed: {result.stderr}\n")
                return False

        except Exception as e:
            print(f"  ❌ Error: {str(e)}\n")
            return False

    def _generate_test_cases(self) -> bool:
        """Step 2: 테스트 케이스를 생성합니다."""
        print("📝 Step 2: Generating Test Cases")

        try:
            result = subprocess.run([
                'python', 'scripts/test_case_generator.py',
                str(self.project_path),
                '--scope-file', str(self.output_dir / 'qa-scope.json')
            ], capture_output=True, text=True, timeout=120)

            if result.returncode == 0:
                self.results['steps']['test_generation'] = 'success'
                print("  ✅ Test case generation complete\n")
                return True
            else:
                print(f"  ❌ Test generation failed: {result.stderr}\n")
                return False

        except Exception as e:
            print(f"  ❌ Error: {str(e)}\n")
            return False

    def _run_tests(self) -> bool:
        """Step 3: Playwright 테스트를 실행합니다."""
        print("🎬 Step 3: Running Tests with Video Recording")

        try:
            # 동영상 녹화 결과를 저장할 recordings 디렉토리 생성
            recordings_dir = self.output_dir / 'recordings'
            recordings_dir.mkdir(parents=True, exist_ok=True)

            # Playwright 테스트 실행
            # playwright.config.ts의 video 설정으로 녹화가 수행되며,
            # 동영상 파일명은 {priority}-{component}-{scenario-slug}.webm 형식으로 적용됨
            result = subprocess.run([
                'npx', 'playwright', 'test',
                '--config', 'playwright.config.ts',
                '--reporter=json',
                f'--project={self.config.get("browser", "chromium")}',
                f'--workers={self.config.get("workers", 4)}'
            ], capture_output=True, text=True, timeout=300)

            # 결과 저장
            test_results = {
                'exit_code': result.returncode,
                'output': result.stdout,
                'errors': result.stderr,
                'recordings_path': str(recordings_dir)
            }

            with open(self.output_dir / 'test-results.json', 'w') as f:
                json.dump(test_results, f, indent=2)

            self.results['steps']['test_execution'] = 'success' if result.returncode == 0 else 'failed'
            print(f"  {'✅' if result.returncode == 0 else '⚠️'} Test execution complete\n")
            return True

        except Exception as e:
            print(f"  ❌ Error: {str(e)}\n")
            return False

    def _analyze_results(self) -> bool:
        """Step 4: 테스트 결과를 분석합니다."""
        print("📊 Step 4: Analyzing Results")

        try:
            # 테스트 결과 로드
            test_results_file = self.output_dir / 'test-results.json'
            if not test_results_file.exists():
                print("  ⚠️ No test results found\n")
                return False

            with open(test_results_file, 'r') as f:
                test_results = json.load(f)

            # 메트릭 계산
            exit_code = test_results.get('exit_code', 1)
            passed = exit_code == 0
            success_rate = 100 if passed else 0

            self.results['metrics'] = {
                'success_rate': success_rate,
                'passed': passed,
                'timestamp': datetime.now().isoformat()
            }

            self.results['success'] = success_rate >= 80

            print(f"  📈 Success Rate: {success_rate}%")
            print(f"  {'✅' if self.results['success'] else '⚠️'} Analysis complete\n")
            return True

        except Exception as e:
            print(f"  ❌ Error: {str(e)}\n")
            return False

    def _generate_automation_scripts(self) -> bool:
        """Step 5: 자동화 스크립트를 생성합니다 (성공 시)."""
        print("🔧 Step 5: Generating Automation Scripts")

        try:
            result = subprocess.run([
                'python', 'scripts/automation_script_generator.py',
                '--qa-result', str(self.output_dir / 'test-results.json'),
                '--output-dir', str(self.output_dir / 'automation-scripts')
            ], capture_output=True, text=True, timeout=120)

            if result.returncode == 0:
                self.results['steps']['automation_generation'] = 'success'
                print("  ✅ Automation scripts generated\n")
                return True
            else:
                print(f"  ⚠️ Automation script generation skipped: {result.stderr}\n")
                return True  # 실패해도 계속 진행

        except Exception as e:
            print(f"  ⚠️ Error generating scripts: {str(e)}\n")
            return True

    def _generate_final_report(self):
        """최종 리포트를 생성합니다."""
        print("📋 Generating Final Report")

        # Summary 생성
        summary = {
            'timestamp': self.results['timestamp'],
            'status': 'SUCCESS' if self.results['success'] else 'FAILED',
            'metrics': self.results['metrics'],
            'steps_completed': self.results['steps'],
            'output_location': str(self.output_dir),
            'next_steps': [
                "Review qa-results/summary.json for detailed metrics",
                "Check qa-results/recordings/ for test videos",
                "If successful, use automation-scripts/ for CI/CD setup"
            ] if self.results['success'] else [
                "Review qa-results/test-results.json for failures",
                "Check logs for error details"
            ]
        }

        # 최종 리포트 저장
        with open(self.output_dir / 'summary.json', 'w') as f:
            json.dump(summary, f, indent=2)

        print(f"\n{'='*50}")
        print(f"QA AUTOMATION ORCHESTRATOR REPORT")
        print(f"{'='*50}")
        print(f"Status: {summary['status']}")
        print(f"Success Rate: {summary['metrics'].get('success_rate', 0)}%")
        print(f"Output Location: {summary['output_location']}")
        print(f"\nNext Steps:")
        for step in summary['next_steps']:
            print(f"  • {step}")
        print(f"{'='*50}\n")

def main():
    # Default config
    config = {
        'project_path': '.',
        'output_dir': './qa-results',
        'browser': 'chromium',
        'workers': 4,
        'record_video': True,
        'auto_generate_scripts': True
    }

    # Command line arguments 처리
    if '--config' in sys.argv:
        idx = sys.argv.index('--config')
        config_file = sys.argv[idx + 1]
        with open(config_file, 'r') as f:
            user_config = json.load(f)
            config.update(user_config)

    # 각 옵션별 처리
    if '--project-path' in sys.argv:
        idx = sys.argv.index('--project-path')
        config['project_path'] = sys.argv[idx + 1]

    if '--output-dir' in sys.argv:
        idx = sys.argv.index('--output-dir')
        config['output_dir'] = sys.argv[idx + 1]

    if '--workers' in sys.argv:
        idx = sys.argv.index('--workers')
        config['workers'] = int(sys.argv[idx + 1])

    orchestrator = QAOrchestrator(config)
    success = orchestrator.run()

    sys.exit(0 if success else 1)

if __name__ == "__main__":
    main()
