#!/usr/bin/env python3
"""
Test Case Generator

QA 범위에서 자동으로 테스트 케이스를 생성합니다.
- Given-When-Then 패턴 적용
- 컴포넌트별 테스트 케이스 생성
- Integration test scenarios 도출
- Playwright E2E 테스트 코드 생성
"""

import json
import re
import sys
from pathlib import Path
from typing import Dict, List, Any
from dataclasses import dataclass

@dataclass
class TestCase:
    id: str
    component: str
    title: str
    priority: str
    scenario: Dict[str, Any]  # Given, When, Then
    test_type: str  # unit, integration, e2e
    playwright_code: str

class TestCaseGenerator:
    def __init__(self, project_path: str, scope_file: str = None):
        self.project_path = Path(project_path)
        self.scope_file = scope_file
        self.scope_data = self._load_scope()
        self.test_cases: List[TestCase] = []

    def _to_kebab_case(self, name: str) -> str:
        """PascalCase/camelCase 문자열을 kebab-case로 변환합니다."""
        kebab = re.sub(r'([a-z0-9])([A-Z])', r'\1-\2', name)
        kebab = re.sub(r'([A-Z]+)([A-Z][a-z])', r'\1-\2', kebab)
        return kebab.lower()

    def _load_scope(self) -> Dict:
        """QA 범위 파일을 로드합니다."""
        if self.scope_file and Path(self.scope_file).exists():
            with open(self.scope_file, 'r') as f:
                return json.load(f)
        return {"components": []}

    def generate(self) -> Dict:
        """테스트 케이스를 생성합니다."""
        print("📝 Generating test cases...")

        components = self.scope_data.get('components', [])
        for component in components:
            self._generate_for_component(component)

        print(f"✅ Generated {len(self.test_cases)} test cases")
        return self._build_report()

    def _generate_for_component(self, component: Dict):
        """컴포넌트별 테스트 케이스를 생성합니다."""
        comp_name = component.get('name', 'Unknown')
        comp_type = component.get('type', 'component')
        priority = component.get('priority', 'MEDIUM')

        # 컴포넌트 타입별 테스트 케이스 생성
        if comp_type == 'component':
            self._generate_component_tests(comp_name, priority)
        elif comp_type == 'page':
            self._generate_page_tests(comp_name, priority)
        elif comp_type == 'service':
            self._generate_service_tests(comp_name, priority)

    def _generate_component_tests(self, component_name: str, priority: str):
        """컴포넌트 테스트 케이스를 생성합니다."""
        test_patterns = [
            {
                'title': f'{component_name} renders correctly',
                'scenario': {
                    'Given': f'User is on a page with {component_name}',
                    'When': 'The component is mounted',
                    'Then': f'The {component_name} is visible and interactive'
                },
                'test_type': 'unit'
            },
            {
                'title': f'{component_name} handles user interactions',
                'scenario': {
                    'Given': f'{component_name} is rendered',
                    'When': 'User interacts with the component',
                    'Then': 'Component responds correctly to interactions'
                },
                'test_type': 'unit'
            },
            {
                'title': f'{component_name} displays error states',
                'scenario': {
                    'Given': f'{component_name} receives invalid data',
                    'When': 'Component processes the data',
                    'Then': 'Error message is displayed appropriately'
                },
                'test_type': 'unit'
            }
        ]

        for idx, pattern in enumerate(test_patterns):
            test_case = TestCase(
                id=f"{component_name}-{idx+1}",
                component=component_name,
                title=pattern['title'],
                priority=priority,
                scenario=pattern['scenario'],
                test_type=pattern['test_type'],
                playwright_code=self._generate_playwright_code(component_name, pattern)
            )
            self.test_cases.append(test_case)

    def _generate_page_tests(self, page_name: str, priority: str):
        """페이지 테스트 케이스를 생성합니다."""
        test_patterns = [
            {
                'title': f'{page_name} page loads successfully',
                'scenario': {
                    'Given': 'User is not on the page',
                    'When': 'User navigates to the page',
                    'Then': 'Page loads with all content visible'
                },
                'test_type': 'e2e'
            },
            {
                'title': f'{page_name} navigation works correctly',
                'scenario': {
                    'Given': f'User is on the {page_name} page',
                    'When': 'User clicks navigation links',
                    'Then': 'User is redirected to correct pages'
                },
                'test_type': 'e2e'
            },
            {
                'title': f'{page_name} responsive design',
                'scenario': {
                    'Given': f'{page_name} is open',
                    'When': 'Window is resized to mobile size',
                    'Then': 'Layout adapts correctly for mobile'
                },
                'test_type': 'e2e'
            }
        ]

        for idx, pattern in enumerate(test_patterns):
            test_case = TestCase(
                id=f"{page_name}-{idx+1}",
                component=page_name,
                title=pattern['title'],
                priority=priority,
                scenario=pattern['scenario'],
                test_type=pattern['test_type'],
                playwright_code=self._generate_playwright_code(page_name, pattern, is_page=True)
            )
            self.test_cases.append(test_case)

    def _generate_service_tests(self, service_name: str, priority: str):
        """서비스 테스트 케이스를 생성합니다."""
        test_patterns = [
            {
                'title': f'{service_name} returns expected data',
                'scenario': {
                    'Given': f'{service_name} is called',
                    'When': 'Request is valid',
                    'Then': 'Service returns correct data structure'
                },
                'test_type': 'unit'
            },
            {
                'title': f'{service_name} handles errors',
                'scenario': {
                    'Given': f'{service_name} receives invalid input',
                    'When': 'Service processes the input',
                    'Then': 'Service throws appropriate error'
                },
                'test_type': 'unit'
            }
        ]

        for idx, pattern in enumerate(test_patterns):
            test_case = TestCase(
                id=f"{service_name}-{idx+1}",
                component=service_name,
                title=pattern['title'],
                priority=priority,
                scenario=pattern['scenario'],
                test_type=pattern['test_type'],
                playwright_code=self._generate_playwright_code(service_name, pattern)
            )
            self.test_cases.append(test_case)

    def _generate_playwright_code(self, name: str, pattern: Dict, is_page: bool = False) -> str:
        """Playwright 테스트 코드를 생성합니다."""
        title = pattern['title']
        scenario = pattern['scenario']

        code = f"""import {{ test, expect }} from '@playwright/test';

// 동영상 녹화는 playwright.config.ts의 use.video 옵션에서 설정
test('{title}', async ({{ page }}) => {{
  // Given: {scenario['Given']}
  await page.goto('https://app.example.com');

  // When: {scenario['When']}
  // TODO: Add interaction steps

  // Then: {scenario['Then']}
  // TODO: Add assertions
}});
"""
        return code

    def _build_report(self) -> Dict:
        """테스트 케이스 리포트를 빌드합니다."""
        test_cases_by_type = {}
        for tc in self.test_cases:
            if tc.test_type not in test_cases_by_type:
                test_cases_by_type[tc.test_type] = []
            test_cases_by_type[tc.test_type].append({
                'id': tc.id,
                'component': tc.component,
                'title': tc.title,
                'priority': tc.priority,
                'scenario': tc.scenario,
                'code': tc.playwright_code
            })

        return {
            'total_test_cases': len(self.test_cases),
            'by_type': {
                'unit': len(test_cases_by_type.get('unit', [])),
                'integration': len(test_cases_by_type.get('integration', [])),
                'e2e': len(test_cases_by_type.get('e2e', [])),
            },
            'test_cases': test_cases_by_type,
            'playwright_files': self._export_playwright_files()
        }

    def _export_playwright_files(self) -> Dict[str, str]:
        """Playwright 테스트 파일을 내보냅니다."""
        files = {}
        for tc in self.test_cases:
            if tc.test_type == 'e2e':
                key = f"tests/qa-e2e/{self._to_kebab_case(tc.component)}.qa-e2e.test.ts"
                if key not in files:
                    files[key] = ""
                files[key] += tc.playwright_code + "\n"
        return files

def main():
    if len(sys.argv) < 2:
        print("Usage: test_case_generator.py <project-path> [--scope-file <scope-file>]")
        sys.exit(1)

    project_path = sys.argv[1]
    scope_file = None

    if "--scope-file" in sys.argv:
        idx = sys.argv.index("--scope-file")
        if idx + 1 < len(sys.argv):
            scope_file = sys.argv[idx + 1]

    generator = TestCaseGenerator(project_path, scope_file)
    result = generator.generate()

    # Save test plan
    with open("qa-test-plan.json", 'w') as f:
        json.dump(result, f, indent=2)

    print(f"\n✅ Test case generation complete!")
    print(f"📄 Test plan saved to: qa-test-plan.json")
    print(f"\n📊 Summary:")
    print(f"  - Total test cases: {result['total_test_cases']}")
    print(f"  - Unit tests: {result['by_type']['unit']}")
    print(f"  - Integration tests: {result['by_type']['integration']}")
    print(f"  - E2E tests: {result['by_type']['e2e']}")

if __name__ == "__main__":
    main()
