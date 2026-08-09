---
name: data-flow-tracer-skill
description: >
  DB 테이블에서 Entity, DTO, API Response까지 데이터 흐름을 추적하고 Mermaid 다이어그램으로 시각화하는 스킬.
  ORM에서 DB 컬럼명과 코드 필드명이 다른 경우(snake_case → camelCase 등) 매핑 관계를 분석한다.
  rg, Serena(find_symbol, find_referencing_symbols), xargs 등을 적극 활용하여 데이터 변환 체인을 끝까지 추적한다.
  JPA, TypeORM, Prisma, SQLAlchemy, Django, MyBatis, GORM 등 주요 ORM 프레임워크를 지원한다.
  트리거 키워드: 데이터흐름, data flow, 테이블매핑, entity mapping, DTO 매핑, 컬럼매핑, column mapping,
  DB 스키마, 데이터변환, data trace, 필드매핑, field mapping, ORM 분석, 테이블에서DTO, table to dto
---

# Data Flow Tracer

DB 테이블 → Entity → DTO → API Response 간의 데이터 흐름과 필드 매핑을 분석하여 Mermaid 다이어그램으로 시각화한다.

## Arguments

`$ARGUMENTS`: 분석 대상 (아래 형식 중 하나)
- **테이블명**: `user_accounts`, `orders`
- **엔티티/모델명**: `UserAccount`, `Order`
- **모듈 경로**: `src/domain/user`, `app/models/order.py`
- **여러 대상**: 공백으로 구분

---

## 참조

- `@.claude/core/MDX.md` (MDX/Mermaid 문법)

---

## 실행 워크플로우

### Phase 1: 프레임워크 감지 및 초기 탐색

**discover-entities.sh 스크립트 실행으로 빠른 전체 파악:**
```bash
bash <skill-path>/scripts/discover-entities.sh <project-root> [target]
```

스크립트 결과를 바탕으로 사용 중인 ORM 프레임워크와 Entity/DTO/Mapper 파일 목록을 확인한다.
상세한 프레임워크별 탐지 패턴은 `references/orm-patterns.md`를 참조.

**Serena 활용:** `get_symbols_overview`로 대상 파일의 심볼 구조를 파악한다.

### Phase 2: DB → Entity 매핑 분석

**목표:** DB 테이블/컬럼명과 코드 엔티티/필드명 간의 매핑 관계를 완전히 파악한다.

**분석 절차:**

1. **테이블명 매핑 확인**
   - `$ARGUMENTS`로 지정된 테이블/엔티티의 정확한 DB 테이블명과 엔티티 클래스명을 확인
   - Serena `find_symbol`로 엔티티 클래스를 찾고 `include_body=True`로 전체 필드 확인
   - 명시적 테이블명 어노테이션이 없으면 프레임워크의 기본 네이밍 전략 추론 (보통 camelCase → snake_case)

2. **컬럼 매핑 수집**
   - 각 필드의 DB 컬럼명과 코드 필드명을 쌍으로 수집
   - 명시적 매핑 (`@Column(name="...")`) 확인
   - 암묵적 매핑 (네이밍 전략에 의한 자동 변환) 추론
   - 타입 변환도 기록 (VARCHAR → Enum, TIMESTAMP → LocalDateTime 등)

3. **관계 매핑 수집**
   - FK, Join 컬럼 등의 관계 어노테이션 수집
   - 관계 타입 (1:1, 1:N, N:M) 식별
   - 연결된 다른 엔티티도 함께 분석 대상에 포함

**핵심 도구 사용법:**
```
# 특정 엔티티의 컬럼 매핑 검색
rg '@Column\(.*name.*=.*"' <entity-file>
rg '@JoinColumn|@ManyToOne|@OneToMany' <entity-file>

# Serena로 엔티티 구조 파악
find_symbol(name_path="EntityName", include_body=True)
find_symbol(name_path="EntityName", depth=1)  # 필드/메서드 목록
```

### Phase 3: Entity → DTO 변환 추적

**목표:** 엔티티 데이터가 DTO로 어떻게 변환되는지, 어떤 필드가 포함/제외/변환되는지 파악한다.

**분석 절차:**

1. **엔티티 참조처 추적**
   - Serena `find_referencing_symbols`로 엔티티 클래스가 사용되는 모든 위치를 추적
   - Service, Mapper, Converter 클래스에서의 사용처에 집중

2. **DTO 클래스 식별**
   - 엔티티와 연관된 DTO/Response/Request 클래스를 모두 찾기
   - 하나의 엔티티에 여러 DTO가 있을 수 있음 (목록용, 상세용, 생성용 등)

3. **변환 로직 분석**
   - Mapper 클래스의 매핑 메서드 확인 (MapStruct `@Mapping`, 수동 매핑 등)
   - `toDto()`, `toResponse()`, `of()`, `from()` 등의 팩토리/변환 메서드 확인
   - Builder 패턴이나 생성자를 통한 매핑 확인
   - 각 DTO 필드가 엔티티의 어떤 필드에서 오는지 대응 관계 수집

4. **필드별 변환 방식 분류**
   - **직접 매핑**: 이름과 타입이 동일
   - **필드명 변경**: 이름만 다름 (예: `createdAt` → `joinDate`)
   - **타입 변환**: 타입이 다름 (예: `Enum` → `String`, `LocalDateTime` → `String`)
   - **계산 필드**: 여러 필드에서 파생 (예: `orders.size()` → `orderCount`)
   - **제외 필드**: DTO에 포함되지 않는 민감 정보 (예: `password`)
   - **중첩 DTO**: 관계 엔티티가 중첩 DTO로 변환

**핵심 도구 사용법:**
```
# 엔티티 사용처 추적
find_referencing_symbols(name_path="EntityName", relative_path="entity-file")

# 매핑 패턴 검색
rg 'toDto|toResponse|from.*Entity|of\(' --type java
rg '@Mapping\(source|@Mapping\(target' --type java

# DTO 클래스 분석
find_symbol(name_path="EntityNameDto", include_body=True)
find_symbol(name_path="EntityNameResponse", include_body=True)
```

### Phase 4: DTO → API Response 추적

**목표:** DTO가 최종적으로 어떤 API 엔드포인트에서 어떤 형태로 반환되는지 파악한다.

**분석 절차:**

1. **Controller/Handler 추적**
   - DTO 클래스의 참조처 중 Controller/Handler 레이어를 식별
   - 반환 타입에 해당 DTO가 포함된 엔드포인트 메서드 수집

2. **응답 래핑 분석**
   - `ResponseEntity<Dto>`, `ApiResponse<Dto>` 등의 래퍼 사용 여부
   - 직렬화 어노테이션 (`@JsonProperty`, `@JsonNaming` 등)에 의한 추가 이름 변환

3. **직렬화 최종 형태 확인**
   - JSON 응답에서의 최종 필드명이 DTO 필드명과 다른지 확인
   - `@JsonProperty("response_field")` 등에 의한 추가 매핑

### Phase 5: Mermaid 다이어그램 생성

다이어그램 템플릿은 `references/mermaid-templates.md`를 참조.

**생성할 다이어그램 목록:**

1. **ER 다이어그램**: 관련 테이블 구조와 관계, 각 컬럼에 Entity 필드명 주석
2. **컬럼 매핑 다이어그램 (flowchart LR)**: DB 컬럼 → Entity 필드 간 1:1 매핑
3. **데이터 변환 흐름 (flowchart TD)**: DB → Entity → DTO → Response 전체 흐름
4. **필드 매핑 테이블**: 각 레이어별 필드명, 타입, 변환 방식을 정리한 마크다운 테이블

### Phase 6: 완전성 검증

**모든 Phase를 완료한 후 반드시 다음을 검증한다:**

- [ ] 대상 테이블의 모든 컬럼이 Entity 필드에 매핑되었는가
- [ ] Entity의 모든 필드가 DTO 포함/제외 여부와 함께 분류되었는가
- [ ] 관계 엔티티(FK)의 변환 방식이 확인되었는가
- [ ] API 엔드포인트와 HTTP 메서드가 명시되었는가
- [ ] 직렬화 시 추가 이름 변환이 있는지 확인되었는가
- [ ] 모든 다이어그램이 MDX 문법에 맞는가

**누락된 항목이 있으면 해당 Phase로 돌아가서 추가 분석을 수행한다.**
**완전히 추적이 끝났다고 확신할 때까지 분석을 종료하지 않는다.**

---

## 출력 형식

**파일명**: `{대상명}-data-flow.mdx`
**위치**: `prd/` 폴더 (없으면 생성)

```markdown
# 데이터 흐름 분석: [대상명]

## 분석 대상
- 테이블: [DB 테이블명]
- 엔티티: [Entity 클래스명] (`파일경로`)
- DTO: [DTO 클래스명 목록] (`파일경로`)
- API: [HTTP메서드 /엔드포인트 경로]

## DB 스키마
(erDiagram - 관련 테이블, 컬럼, 관계, Entity 필드명 주석)

## DB → Entity 컬럼 매핑
(flowchart LR - DB 컬럼과 Entity 필드 간 매핑)

### 컬럼 매핑 상세
| DB Column | DB Type | Entity Field | Entity Type | 매핑 방식 |
|-----------|---------|-------------|-------------|----------|

## Entity → DTO 변환
(flowchart LR - Entity 필드와 DTO 필드 간 매핑)

### 변환 상세
| Entity Field | Entity Type | DTO Field | DTO Type | 변환 방식 |
|-------------|-------------|-----------|----------|----------|

### 제외 필드
| Entity Field | 제외 사유 |
|-------------|----------|

## 전체 데이터 흐름
(flowchart TD - DB → Repository → Entity → Service → DTO → Controller → Response)

## DTO → API Response
### 직렬화 매핑
| DTO Field | JSON Key | 비고 |
|-----------|----------|------|

### API 엔드포인트
| 메서드 | 경로 | 요청 DTO | 응답 DTO |
|--------|------|----------|----------|
```

---

## 제약 사항

- 모든 설명과 레이블은 **한국어**로 작성
- H 태그에 **숫자 표기 금지** (예: `## 1. ...` X → `## ...` O)
- 다이어그램당 노드 수 **20개 이내**
- 코드에 존재하는 정보만 기술, 추측 금지 (확인 불가 시 "코드에서 확인 불가" 명시)
- MDX 문법 준수 (`@.claude/core/MDX.md` 참조)
- 분석 대상이 여러 테이블/엔티티에 걸쳐 있으면 각각 별도 섹션으로 분리
