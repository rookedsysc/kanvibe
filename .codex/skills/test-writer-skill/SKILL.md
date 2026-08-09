---
name: test-writer-skill
description: Comprehensive test code generation for Spring Boot/JUnit, NestJS/TypeScript, and Playwright E2E projects with Given-When-Then pattern. Use when the user requests test code creation, test coverage analysis, or asks to test specific code sections. Supports argument-based selective testing, staged changes analysis, framework-specific integration test setup guidance, and E2E testing with Playwright (headless mode, configurable batch size/workers for parallel execution).
---

# Test Writer Skill

자동화된 테스트 코드 작성 및 커버리지 분석을 지원하는 스킬이다.

## Workflow

### 1. 테스트 요청 분석

사용자의 테스트 요청을 분석하고 명확히 한다.

**모든 경우 공통:**
- Git diff를 확인하여 변경된 코드를 파악한다 (staged/unstaged 모두)
- 변경사항 중 테스트가 필요한 부분을 판단한다

**ARGUMENTS가 제공된 경우:**
- ARGUMENTS에 명시된 부분을 우선 테스트 대상으로 간주한다
- 예: "OrderService의 createOrder 메서드만 테스트해줘"
- Diff 분석 결과와 비교하여 ARGUMENTS 범위 외 추가로 테스트가 필요한 부분이 있는지 확인한다

**ARGUMENTS가 없는 경우:**
- Diff 분석 결과를 기반으로 테스트 대상을 제안한다
- 사용자에게 테스트 범위를 명확히 확인한다

### 2. Changes 분석 (공통 로직)

모든 테스트 요청에서 **base branch 대비 변경사항**과 **staged/unstaged 변경사항**을 모두 분석한다.

**Base Branch 탐색 및 브랜치 단위 변경사항 확인:**

`.claude/scripts/find_base_branch.sh`를 실행하여 현재 브랜치의 base branch를 자동 탐색한다. 이 스크립트는 `origin/` 리모트 브랜치 중 현재 HEAD의 조상이면서 가장 가까운 브랜치를 찾아 반환한다.

```bash
# base branch 탐색
BASE_BRANCH=$(bash .claude/scripts/find_base_branch.sh)

# base branch 대비 전체 변경 파일 목록
git diff --name-only "$BASE_BRANCH"...HEAD

# base branch 대비 변경 내용 확인
git diff "$BASE_BRANCH"...HEAD
```

**Staged와 Unstaged 변경사항 확인 (추가):**

base branch 대비 변경사항 외에, 아직 커밋되지 않은 작업 중인 코드도 확인한다.

```bash
# Staged 변경사항
git diff --staged

# Unstaged 변경사항 (working directory)
git diff
```

base branch 대비 변경사항과 staged/unstaged 변경사항을 종합하여, 아래 기준에 따라 테스트 필요성을 판단한다.

**반드시 테스트해야 할 변경사항 판단 기준:**

다음 중 하나라도 해당하면 테스트가 필수다:

1. **비즈니스 로직 변경**
   - 계산 로직 (금액, 할인율, 수수료 등)
   - 상태 전환 로직 (주문 상태, 결제 상태 등)
   - 조건부 분기가 포함된 로직

2. **외부 시스템 연동 코드**
   - API 클라이언트 호출
   - 데이터베이스 쿼리 (복잡한 JOIN, 집계 등)
   - 메시지 큐 발행/구독

3. **보안 관련 변경**
   - 인증/인가 로직
   - 입력값 검증
   - 권한 체크

4. **데이터 일관성 관련**
   - 트랜잭션 처리
   - 동시성 제어
   - 데이터 검증 규칙

**테스트 불필요 판단 기준:**

다음은 테스트가 필수가 아니다:

- 단순 Getter/Setter 추가
- 상수 정의
- 로그 메시지 변경
- 주석 추가/수정
- 코드 포맷팅 변경

**사용자 피드백:**

변경사항 분석 후, 반드시 테스트가 필요한 항목과 선택적 테스트 항목을 구분하여 사용자에게 제시한다:

```
다음 변경사항을 발견했습니다:

[반드시 테스트 필요]
- OrderService.calculateTotalPrice(): 할인율 계산 로직 변경
- PaymentValidator.validateCard(): 카드 검증 규칙 추가

[선택적 테스트]
- OrderDto: 새로운 필드 추가 (deliveryNote)
- OrderRepository: 단순 조회 메서드 추가

반드시 테스트가 필요한 항목들을 테스트하시겠습니까?
추가로 선택적 항목도 테스트하시겠습니까?
```

### 3. 프레임워크 감지

프로젝트의 테스트 프레임워크를 감지한다.

**Spring Boot + JUnit 감지:**
- `pom.xml` 또는 `build.gradle`에서 `spring-boot-starter-test` 확인
- `src/test/java` 디렉토리 존재 확인
- `@SpringBootTest`, `@WebMvcTest` 등의 어노테이션 사용 확인

**Spring Boot 프로젝트로 감지되면:**
- [references/spring-boot-junit.md](references/spring-boot-junit.md)를 로드한다

**NestJS + TypeScript 감지:**
- `package.json`에서 `@nestjs/testing`, `jest` 확인
- `test/` 디렉토리 또는 `.spec.ts` 파일 확인
- `describe`, `it` 패턴 사용 확인

**NestJS 프로젝트로 감지되면:**
- [references/nestjs-typescript.md](references/nestjs-typescript.md)를 로드한다

**Playwright E2E 테스트 감지:**
- `package.json`에서 `@playwright/test` 확인
- `playwright.config.ts` 파일 존재 확인
- `tests/` 또는 `e2e/` 디렉토리에 `.spec.ts` 파일 확인

**Playwright 프로젝트로 감지되면:**
- [references/playwright-e2e.md](references/playwright-e2e.md)를 로드한다

### 4. 테스트 환경 설정 확인

**Spring Boot 프로젝트의 경우:**
1. `src/test/resources/application.yaml` 읽기
2. Test Container vs 전용 Test DB 판단
3. Integration Test 가이드는 이미 로드한 spring-boot-junit.md를 참조

**NestJS 프로젝트의 경우:**
1. `package.json`의 test scripts 확인
2. `test/database.config.ts` 또는 환경 변수 확인
3. E2E Test 설정은 이미 로드한 nestjs-typescript.md를 참조

**Playwright E2E 프로젝트의 경우:**
1. `playwright.config.ts`에서 baseURL, headless 모드, workers 설정 확인
2. 테스트 데이터 초기화 스크립트 확인 (setup/teardown)
3. E2E Test 가이드는 이미 로드한 playwright-e2e.md를 참조

### 5. 테스트 코드 작성

**필수 준수 사항:**

1. **테스트 독립성 보장**
   - 각 테스트는 다른 테스트에 의존하지 않고 단독으로 실행 가능해야 한다
   - 테스트 실행 순서에 따라 결과가 달라지면 안 된다
   - 공유 상태(전역 변수, DB 데이터, 파일 등)를 테스트 간에 공유하지 않는다
   - 각 테스트는 자신의 Given 단계에서 필요한 데이터를 직접 셋업한다
   - `@BeforeEach`/`beforeEach` 등 초기화 훅을 활용하여 테스트 간 상태를 격리한다

2. **Given-When-Then 패턴 사용**
   - 모든 테스트는 Given-When-Then 구조를 따른다
   - 주석으로 각 섹션을 명확히 구분한다

3. **한국어 테스트 메서드명 또는 한국어 설명**
   - JUnit: `@DisplayName("재고가 충분할 때 주문을 생성한다")`
   - Jest/NestJS: `it('재고가 충분할 때 주문을 생성한다', async () => { ... })`

4. **프레임워크별 패턴 준수**
   - 프레임워크 감지 시 로드한 레퍼런스 문서를 반드시 참조한다

**테스트 작성 순서:**

1. **ARGUMENTS로 명시된 부분 테스트**
   - 사용자가 요청한 정확한 범위만 구현한다

2. **추가 테스트 제안**
   - 작성한 테스트 외에 추가로 필요한 테스트가 있는지 분석한다
   - 누락된 엣지 케이스, 예외 케이스를 사용자에게 제안한다
   - 예: "InvalidQuantityException 케이스도 테스트하시겠습니까?"

3. **사용자 확인 후 추가 구현**
   - 사용자가 동의한 추가 테스트만 구현한다

### 6. 테스트 실행 가이드

프레임워크 감지 시 로드한 레퍼런스 문서의 테스트 실행 명령어 섹션을 참조하여 제공한다.

## 금지 사항

1. **사용자가 요청하지 않은 범위를 임의로 테스트하지 않는다**
   - ARGUMENTS가 "createOrder 메서드만"이라면, updateOrder는 테스트하지 않는다

2. **Given-When-Then 패턴을 생략하지 않는다**
   - 모든 테스트는 반드시 세 섹션으로 나눈다

3. **테스트 환경 설정을 확인하지 않고 코드를 작성하지 않는다**
   - application.yaml이나 package.json을 먼저 확인한다

4. **프레임워크별 레퍼런스를 무시하지 않는다**
   - Spring Boot 프로젝트: 프레임워크 감지 시 로드한 spring-boot-junit.md를 반드시 참조한다
   - NestJS 프로젝트: 프레임워크 감지 시 로드한 nestjs-typescript.md를 반드시 참조한다
   - Playwright 프로젝트: 프레임워크 감지 시 로드한 playwright-e2e.md를 반드시 참조한다

## 사용자 인터랙션 요약

| 요청 유형 | 처리 흐름 |
|----------|----------|
| 명확한 대상 지정 | 프레임워크 감지 → 환경 확인 → 테스트 작성 → 추가 케이스 제안 |
| "테스트 작성해줘" (범위 불명확) | Changes 분석 → 필수/선택 항목 분류 → 사용자 확인 → 테스트 작성 |
| "변경사항 기반 테스트" | base branch 대비 diff + staged/unstaged 분석 → 분류 → 사용자 확인 → 테스트 작성 |
