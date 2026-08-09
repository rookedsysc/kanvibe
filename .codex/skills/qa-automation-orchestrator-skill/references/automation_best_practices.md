# QA 자동화 모범 사례

## 스크립트 작성 원칙

### 1. 테스트 격리
- 각 테스트는 독립적으로 실행 가능해야 함
- 테스트 순서가 결과에 영향을 주면 안 됨
- 각 테스트가 자신의 데이터를 준비하고 정리

### 2. 명확한 테스트 이름
```javascript
// ❌ 나쁜 예
test('login works', () => {})

// ✅ 좋은 예
test('유효한 자격증명으로 로그인하면 대시보드로 이동', () => {})
```

### 3. Given-When-Then 패턴
```javascript
test('로그인', async ({ page }) => {
  // Given: 사용자가 로그인 페이지에 있음
  await page.goto('/login');

  // When: 유효한 자격증명을 입력하고 제출
  await page.fill('#email', 'user@example.com');
  await page.fill('#password', 'password123');
  await page.click('button[type="submit"]');

  // Then: 대시보드로 이동
  await expect(page).toHaveURL('/dashboard');
});
```

### 4. 선택자 전략
```javascript
// 우선순위 순서:
1. getByRole() - 접근성 기반
2. getByLabel() - 라벨 기반
3. getByText() - 텍스트 기반
4. getByTestId() - 마지막 수단
```

### 5. 대기 처리
```javascript
// ❌ 하드코딩된 대기
await page.waitForTimeout(5000);

// ✅ 조건 기반 대기
await expect(page.locator('.loader')).not.toBeVisible();
await page.waitForURL('**/dashboard');
```

## 테스트 분리

일반 테스트와 QA E2E 테스트의 분리 설정은 **[qa_e2e_setup_guide.md](qa_e2e_setup_guide.md)**를 참고합니다.

## CI/CD 통합

### GitHub Actions 설정
```yaml
- name: Run QA Automation
  run: python scripts/qa_orchestrator.py . --workers 4

- name: Upload artifacts
  uses: actions/upload-artifact@v3
  with:
    name: qa-results
    path: qa-results/
    retention-days: 30
```

### 성능 최적화
- 병렬 실행 워커 설정: CPU 코어 수 고려
- CI에서는 1-2개 워커로 안정성 우선
- 로컬에서는 4-8개 워커로 속도 우선

## 트러블슈팅

### Flaky 테스트 해결
1. Trace 파일 분석: `npx playwright show-trace`
2. 대기 조건 추가
3. 네트워크 모킹 추가
4. 상태 관리 확인

### 메모리 누수
```bash
# 프로세스 모니터링
watch -n 1 'ps aux | grep playwright'

# 메모리 제한
python -m memory_profiler scripts/qa_orchestrator.py
```

## 모니터링

### 커버리지 추적
```bash
# 커버리지 리포트 생성
npx playwright test --reporter=coverage

# 커버리지 임계값 설정
threshold: {
  lines: 80,
  branches: 75,
  functions: 80
}
```

### 성공률 추적
- 일일 성공률 대시보드
- 주간 추세 분석
- Critical 테스트 100% 목표

## 보안 고려사항

### 민감한 정보 처리
```javascript
// ❌ 테스트에 하드코딩하지 말 것
const password = 'mypassword123';

// ✅ 환경 변수 사용
const password = process.env.TEST_PASSWORD;
```

### 테스트 데이터
- 테스트용 별도 계정 사용
- 프로덕션 데이터 접근 금지
- 테스트 후 데이터 정리

## Playwright 의존성 관리

설치 및 설정 방법은 **[qa_e2e_setup_guide.md](qa_e2e_setup_guide.md)**를 참고합니다. 버전 관리 시 `package-lock.json`을 커밋하여 팀 전체에서 동일한 버전을 사용합니다.
