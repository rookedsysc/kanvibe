# NestJS + TypeScript Testing Guide

## Test Configuration Detection

코드를 작성하기 전에 반드시 테스트 환경 설정을 확인한다.

### 1. package.json 확인

필수 확인 항목:
- `scripts` 내 test 명령어 (`test`, `test:watch`, `test:cov`, `test:e2e`)
- `devDependencies` 내 `@nestjs/testing`, `jest`, `supertest` 버전

### 2. Database Configuration 확인

- TypeORM: `test/database.config.ts` 또는 환경 변수 기반 설정 확인
- Docker Compose: `docker-compose.test.yml` 존재 여부 확인

## 테스트 컨벤션

### 테스트 유형별 구조

| 테스트 유형 | 모듈 설정 | Mock 방식 |
|-----------|----------|----------|
| Service Unit | `Test.createTestingModule` + `useValue` mock | `jest.fn()` 직접 주입 |
| Controller Unit | `Test.createTestingModule` + `ValidationPipe` | Service를 `useValue`로 mock |
| E2E | `AppModule` import + Test DB 설정 | 실제 모듈, `beforeEach`에서 데이터 정리 |

### Given-When-Then 주석

모든 테스트 내부에 `// given`, `// when`, `// then` 주석으로 섹션을 구분한다.

### 한국어 테스트 설명

```typescript
it('재고가 충분할 때 주문을 생성한다', async () => {
```

- `describe`: 테스트 대상 클래스/메서드명
- `it`: 한국어로 비즈니스 행위를 서술

### Mock 사용 원칙

- **Service Unit Test**: `jest.fn()`으로 Repository/Client를 mock하고 `useValue`로 주입
- **Controller Unit Test**: Service를 `jest.fn()`으로 mock, `app.useGlobalPipes(new ValidationPipe())` 설정
- **E2E Test**: `beforeEach`에서 `repository.clear()`로 테스트 간 데이터 격리
- **External API**: `jest.spyOn(httpService, 'get').mockReturnValue(of(...))`
- **Event Emitter**: `expect(eventEmitter.emit).toHaveBeenCalledWith('event.name', expect.objectContaining({...}))`

### Lifecycle 관리

- `afterEach`에서 `app.close()` 호출 (Controller/E2E Test)
- E2E는 `beforeAll`에서 app 초기화, `afterAll`에서 종료
- `beforeEach`에서 테스트 간 데이터 정리

## 테스트 범위 우선순위

### 반드시 테스트해야 할 코드 (High Priority)

1. **비즈니스 로직 핵심 규칙** - 계산, 상태 전환, 권한 검증
2. **외부 시스템 연동** - HTTP Client, 외부 API, 메시지 큐
3. **데이터 일관성** - 트랜잭션 처리, 동시성 제어, 데이터 검증
4. **보안 관련** - Guards (인증/인가), Validators, Pipes

### 선택적 테스트 (Medium Priority)

- 단순 CRUD (Repository 기본 조회/저장, DTO 변환)
- Module 설정, Provider 등록

### 테스트 불필요 (Low Priority)

- Getter/Setter, 상수 정의, 단순 위임 메서드

## 테스트 실행 명령어

```bash
# 전체 테스트 실행
npm test

# 특정 파일 테스트
npm test order.service.spec.ts

# 커버리지 리포트 생성
npm run test:cov

# E2E 테스트
npm run test:e2e
```
