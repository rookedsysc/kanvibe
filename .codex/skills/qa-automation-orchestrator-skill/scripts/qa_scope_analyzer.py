#!/usr/bin/env python3
"""
QA Scope Analyzer

프로젝트 구조를 분석하여 QA 테스트 범위를 자동으로 확정합니다.
- 컴포넌트 자동 검출
- 우선순위 산정 (Critical, High, Medium, Low)
- 의존성 분석
- 기존 테스트 커버리지 분석
"""

import os
import json
import sys
from pathlib import Path
from typing import Dict, List, Set
import re
from dataclasses import dataclass, asdict
from enum import Enum

class Priority(Enum):
    CRITICAL = "CRITICAL"
    HIGH = "HIGH"
    MEDIUM = "MEDIUM"
    LOW = "LOW"

@dataclass
class Component:
    name: str
    path: str
    type: str  # 'component', 'page', 'service', 'utility'
    priority: Priority
    dependencies: List[str]
    has_tests: bool
    test_coverage: float

class QAScopeAnalyzer:
    def __init__(self, project_path: str):
        self.project_path = Path(project_path)
        self.components: Dict[str, Component] = {}
        self.critical_paths: Set[str] = set()

    def analyze(self) -> Dict:
        """프로젝트를 분석하고 QA 범위를 확정합니다."""
        print(f"🔍 Analyzing project structure: {self.project_path}")

        # 1. 컴포넌트 검출
        self._detect_components()
        print(f"📦 Found {len(self.components)} components")

        # 2. 우선순위 산정
        self._calculate_priorities()
        print("⭐ Calculated priorities")

        # 3. 의존성 분석
        self._analyze_dependencies()
        print("🔗 Analyzed dependencies")

        # 4. 테스트 커버리지 확인
        self._check_test_coverage()
        print("✅ Checked test coverage")

        return self._generate_scope_report()

    def _detect_components(self):
        """컴포넌트를 자동으로 검출합니다."""
        patterns = {
            'component': ['components/**/*.tsx', 'components/**/*.jsx', 'src/components/**/*'],
            'page': ['pages/**/*.tsx', 'pages/**/*.jsx', 'src/pages/**/*'],
            'service': ['services/**/*.ts', 'src/services/**/*.ts'],
            'utility': ['utils/**/*.ts', 'src/utils/**/*.ts'],
        }

        for comp_type, patterns_list in patterns.items():
            for pattern in patterns_list:
                full_pattern = self.project_path / pattern
                for file_path in full_pattern.parent.glob(f"{full_pattern.name.split('/')[-1]}"):
                    if file_path.is_file():
                        name = file_path.stem
                        self.components[name] = Component(
                            name=name,
                            path=str(file_path.relative_to(self.project_path)),
                            type=comp_type,
                            priority=Priority.MEDIUM,
                            dependencies=[],
                            has_tests=False,
                            test_coverage=0.0,
                        )

    def _calculate_priorities(self):
        """컴포넌트 우선순위를 산정합니다."""
        # 우선순위 결정 로직
        critical_keywords = ['auth', 'login', 'payment', 'checkout', 'transaction']
        high_keywords = ['form', 'input', 'button', 'modal', 'dialog']

        for component in self.components.values():
            name_lower = component.name.lower()

            # Critical 판정
            if any(keyword in name_lower for keyword in critical_keywords):
                component.priority = Priority.CRITICAL
            # High 판정
            elif any(keyword in name_lower for keyword in high_keywords):
                component.priority = Priority.HIGH
            # 페이지는 HIGH
            elif component.type == 'page':
                component.priority = Priority.HIGH

    def _analyze_dependencies(self):
        """컴포넌트 간 의존성을 분석합니다."""
        # 간단한 의존성 분석 (import 문 분석)
        for component in self.components.values():
            try:
                file_path = self.project_path / component.path
                if file_path.exists():
                    with open(file_path, 'r', encoding='utf-8', errors='ignore') as f:
                        content = f.read()
                        # import 문에서 의존성 추출
                        imports = re.findall(r"from ['\"](\.+/.+)['\"]|import ['\"](\.+/.+)['\"]", content)
                        component.dependencies = [imp[0] or imp[1] for imp in imports]
            except Exception as e:
                pass  # 파일 읽기 실패 무시

    def _check_test_coverage(self):
        """기존 테스트 커버리지를 확인합니다."""
        test_patterns = [
            f"{self.project_path}/tests/**/*.test.ts",
            f"{self.project_path}/tests/**/*.spec.ts",
            f"{self.project_path}/__tests__/**/*.test.ts",
        ]

        test_files = set()
        for pattern in test_patterns:
            test_files.update(Path(pattern.split('*')[0]).glob('**/*.test.ts'))
            test_files.update(Path(pattern.split('*')[0]).glob('**/*.spec.ts'))

        # 테스트 파일과 컴포넌트 매칭
        for test_file in test_files:
            for component in self.components.values():
                if component.name in test_file.name:
                    component.has_tests = True
                    component.test_coverage = 0.7  # 가정

    def _generate_scope_report(self) -> Dict:
        """QA 범위 리포트를 생성합니다."""
        # 우선순위별로 정렬
        sorted_components = sorted(
            self.components.values(),
            key=lambda x: (x.priority.value, x.name)
        )

        report = {
            "timestamp": str(Path.cwd()),
            "total_components": len(self.components),
            "components": [asdict(c) for c in sorted_components],
            "by_priority": {
                "CRITICAL": [c.name for c in sorted_components if c.priority == Priority.CRITICAL],
                "HIGH": [c.name for c in sorted_components if c.priority == Priority.HIGH],
                "MEDIUM": [c.name for c in sorted_components if c.priority == Priority.MEDIUM],
                "LOW": [c.name for c in sorted_components if c.priority == Priority.LOW],
            },
            "coverage": {
                "total": len(self.components),
                "with_tests": sum(1 for c in self.components.values() if c.has_tests),
                "without_tests": sum(1 for c in self.components.values() if not c.has_tests),
            }
        }

        return report

def main():
    if len(sys.argv) < 2:
        print("Usage: qa_scope_analyzer.py <project-path> [--output <output-file>]")
        sys.exit(1)

    project_path = sys.argv[1]
    output_file = "qa-scope-result.json"

    # Parse arguments
    if "--output" in sys.argv:
        idx = sys.argv.index("--output")
        if idx + 1 < len(sys.argv):
            output_file = sys.argv[idx + 1]

    analyzer = QAScopeAnalyzer(project_path)
    result = analyzer.analyze()

    # Save result
    with open(output_file, 'w') as f:
        json.dump(result, f, indent=2, default=str)

    print(f"\n✅ Scope analysis complete!")
    print(f"📄 Results saved to: {output_file}")
    print(f"\n📊 Summary:")
    print(f"  - Total components: {result['total_components']}")
    print(f"  - Critical: {len(result['by_priority']['CRITICAL'])}")
    print(f"  - High: {len(result['by_priority']['HIGH'])}")
    print(f"  - Medium: {len(result['by_priority']['MEDIUM'])}")
    print(f"  - Low: {len(result['by_priority']['LOW'])}")

if __name__ == "__main__":
    main()
