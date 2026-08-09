# Spring Boot + JUnit Testing Guide

## Test Configuration Detection

코드를 작성하기 전에 반드시 테스트 환경 설정을 확인한다.

### 1. application.yaml 확인

테스트 환경 설정 파일 위치를 확인한다:
- `src/test/resources/application.yaml`
- `src/test/resources/application.yml`
- `src/test/resources/application-test.yaml`

### 2. Test Container vs Dedicated Test DB 판단 기준

**Test Container 사용 징후:**
- `org.testcontainers.jdbc.ContainerDatabaseDriver` 드라이버
- `jdbc:tc:` 접두사 URL
- `@Testcontainers`, `@Container` 어노테이션

**전용 Test DB 사용 징후:**
- 별도 포트/호스트의 DB URL (예: `localhost:5433/test_db`)

### 3. Integration Test 어노테이션 가이드

| 테스트 유형 | 필수 어노테이션 |
|-----------|--------------|
| Test Container | `@SpringBootTest` + `@Testcontainers` + `@DynamicPropertySource` |
| 전용 Test DB | `@SpringBootTest` + `@Transactional` |
| Controller 단위 | `@WebMvcTest(TargetController.class)` |
| Service 단위 | `@ExtendWith(MockitoExtension.class)` |

## 테스트 컨벤션

### Given-When-Then 주석

모든 테스트 메서드 내부에 `// given`, `// when`, `// then` 주석으로 섹션을 구분한다.

### 한국어 DisplayName

```java
@Test
@DisplayName("재고가 충분할 때 주문을 생성한다")
void createOrder_withSufficientStock_shouldCreateOrder() {
```

- `@DisplayName`: 한국어로 비즈니스 행위를 서술
- 메서드명: 영문 camelCase로 `대상_조건_기대결과` 패턴

### Mock 사용 원칙

- **Unit Test**: `@ExtendWith(MockitoExtension.class)` + `@Mock` + `@InjectMocks`
- **Controller Test**: `@WebMvcTest` + `@MockBean`
- **Integration Test**: 실제 Bean 사용, 외부 시스템만 Mock

### Assertion 스타일

- AssertJ 사용: `assertThat(result).isEqualTo(expected)`
- MockMvc: `andExpect(status().isCreated())` + `andExpect(jsonPath("$.field").value(...))`

## 테스트 범위 우선순위

### 반드시 테스트해야 할 코드 (High Priority)

1. **비즈니스 로직 핵심 규칙** - 계산, 상태 전환, 권한 검증
2. **외부 시스템 연동** - Payment Gateway, 외부 API, 메시지 큐
3. **데이터 일관성** - 트랜잭션 경계, 동시성 제어, 데이터 검증
4. **보안 관련** - 인증/인가, 입력값 검증

### 선택적 테스트 (Medium Priority)

- 단순 CRUD (Repository 기본 조회/저장, DTO 변환)
- Configuration 클래스, Bean 정의

### 테스트 불필요 (Low Priority)

- Getter/Setter, 상수 정의, 단순 위임 메서드

## 테스트 실행 명령어

```bash
# 전체 테스트 실행
./gradlew test

# 특정 테스트 클래스 실행
./gradlew test --tests OrderServiceTest

# 커버리지 리포트 생성
./gradlew test jacocoTestReport
```
