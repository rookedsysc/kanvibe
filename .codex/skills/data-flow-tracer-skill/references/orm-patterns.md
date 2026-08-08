# ORM Framework Detection & Analysis Patterns

프레임워크별 엔티티, 컬럼 매핑, DTO 변환 패턴을 정리한 레퍼런스.
각 프레임워크에서 DB 이름과 코드 이름이 달라지는 지점을 빠르게 찾기 위한 rg/Serena 패턴을 포함한다.

---

## JPA / Hibernate (Java/Kotlin)

### Entity → DB Table 매핑
```
rg 패턴: @Table\s*\(.*name\s*=\s*"([^"]+)"
Serena: find_symbol로 @Entity 어노테이션이 있는 클래스 검색
```

**주요 어노테이션:**
| 어노테이션 | 역할 | 예시 |
|-----------|------|------|
| `@Entity` | JPA 엔티티 선언 | `@Entity` |
| `@Table(name="...")` | DB 테이블명 지정 | `@Table(name="user_accounts")` |
| `@Column(name="...")` | DB 컬럼명 지정 | `@Column(name="created_at")` |
| `@JoinColumn(name="...")` | FK 컬럼명 | `@JoinColumn(name="user_id")` |
| `@OneToMany` / `@ManyToOne` | 관계 매핑 | `@ManyToOne(fetch=LAZY)` |
| `@Enumerated` | Enum 저장 방식 | `@Enumerated(EnumType.STRING)` |
| `@Embedded` / `@Embeddable` | 값 객체 내장 | 주소, 금액 등 |

**DTO 변환 패턴:**
```
rg 패턴: toDto|toResponse|from.*Entity|of\(.*entity|MapStruct
Serena: find_referencing_symbols로 Entity 클래스 사용처 추적
```

**MapStruct 매핑:**
```
rg 패턴: @Mapper.*componentModel|@Mapping\(source.*target
```

---

## TypeORM (TypeScript)

### Entity → DB Table 매핑
```
rg 패턴: @Entity\(['\"]([^'\"]*)['\"]|@Entity\(\)
rg 패턴: @Column\(\{.*name:\s*['\"]([^'\"]+)
```

**주요 데코레이터:**
| 데코레이터 | 역할 |
|-----------|------|
| `@Entity("table_name")` | 테이블명 지정 |
| `@Column({ name: "col_name" })` | 컬럼명 지정 |
| `@PrimaryGeneratedColumn()` | Auto-increment PK |
| `@ManyToOne(() => Entity)` | N:1 관계 |
| `@JoinColumn({ name: "fk_col" })` | FK 컬럼 |
| `@CreateDateColumn()` | 자동 생성일 |

**DTO 변환 패턴:**
```
rg 패턴: class.*Dto|class.*Response|plainToClass|classToPlain|instanceToPlain
rg 패턴: @Exclude\(\)|@Expose\(\)|@Transform\(
```

---

## Prisma (TypeScript)

### Model → DB Table 매핑
```
schema.prisma 파일에서:
rg 패턴: model\s+(\w+)|@@map\("([^"]+)"|@map\("([^"]+)"
```

**주요 문법:**
| 문법 | 역할 |
|------|------|
| `model User` | 모델 선언 (기본: 테이블명 = 모델명) |
| `@@map("user_accounts")` | 실제 DB 테이블명 |
| `@map("created_at")` | 실제 DB 컬럼명 |
| `@relation` | 관계 정의 |
| `@id @default(autoincrement())` | PK |

**DTO 변환 패턴:**
Prisma는 자동 생성된 타입(`Prisma.UserCreateInput`)을 사용하거나 수동 DTO 변환.
```
rg 패턴: Prisma\.\w+Create|Prisma\.\w+Update|Prisma\.\w+Select
```

---

## SQLAlchemy (Python)

### Model → DB Table 매핑
```
rg 패턴: __tablename__\s*=\s*['\"]([^'\"]+)
rg 패턴: Column\(['\"]([^'\"]+)
rg 패턴: mapped_column\(.*name=['\"]([^'\"]+)
```

**주요 패턴:**
| 패턴 | 역할 |
|------|------|
| `__tablename__ = "users"` | 테이블명 |
| `Column("db_col", ...)` | 컬럼명 (첫 인자가 DB명) |
| `mapped_column(name="db_col")` | SQLAlchemy 2.0 스타일 |
| `relationship("Entity")` | 관계 정의 |
| `ForeignKey("table.col")` | FK 정의 |

**Pydantic DTO 변환:**
```
rg 패턴: class.*\(BaseModel\)|class.*Schema|model_validate|from_orm
rg 패턴: Field\(.*alias=['\"]|model_config.*from_attributes
```

---

## Django (Python)

### Model → DB Table 매핑
```
rg 패턴: class Meta:.*\n.*db_table\s*=\s*['\"]([^'\"]+)
rg 패턴: db_column=['\"]([^'\"]+)
```

**주요 패턴:**
| 패턴 | 역할 |
|------|------|
| `class Meta: db_table = "..."` | 테이블명 |
| `db_column="col_name"` | 컬럼명 |
| `ForeignKey("app.Model")` | FK |
| `ManyToManyField` | N:M 관계 |

**Serializer (DRF):**
```
rg 패턴: class.*Serializer\(|source=['\"]|SerializerMethodField
```

---

## Sequelize (JavaScript/TypeScript)

### Model → DB Table 매핑
```
rg 패턴: tableName:\s*['\"]([^'\"]+)|modelName:\s*['\"]
rg 패턴: field:\s*['\"]([^'\"]+)
```

---

## MyBatis (Java)

### XML Mapper
```
find 패턴: find . -name "*.xml" -path "*/mapper*"
rg 패턴 (XML): resultMap|column=|property=|<select|<insert
```

**주요 매핑:**
| 패턴 | 역할 |
|------|------|
| `<resultMap>` | DB 컬럼 → Java 필드 매핑 |
| `column="db_col" property="javaField"` | 개별 필드 매핑 |
| `<association>` | N:1 매핑 |
| `<collection>` | 1:N 매핑 |

---

## GORM (Go)

### Model → DB Table 매핑
```
rg 패턴: gorm:"column:([^;"]+)|TableName\(\).*return\s*"([^"]+)"
```

**주요 태그:**
| 태그 | 역할 |
|------|------|
| `` gorm:"column:col_name" `` | 컬럼명 |
| `` gorm:"primaryKey" `` | PK |
| `` gorm:"foreignKey:FieldName" `` | FK |
| `func (m Model) TableName() string` | 커스텀 테이블명 |

---

## 공통 분석 전략

### DB → Entity 매핑 발견 순서
1. 테이블명 지정 패턴 검색 (`@Table`, `__tablename__`, `@@map` 등)
2. 명시적 컬럼 매핑 검색 (`@Column(name=...)`, `@map(...)` 등)
3. 암묵적 네이밍 전략 확인 (camelCase → snake_case 등)
4. 관계 매핑 검색 (FK, Join 등)

### Entity → DTO 매핑 발견 순서
1. Mapper/Converter 클래스 검색
2. Entity 클래스의 참조처(referencing symbols) 추적
3. toDto/toResponse/of 패턴의 팩토리 메서드 검색
4. Builder 패턴이나 생성자 매핑 검색
5. 자동 매핑 라이브러리 설정 검색 (MapStruct, ModelMapper 등)

### DTO → API Response 매핑 발견 순서
1. Controller/Handler에서 DTO 반환 추적
2. 직렬화 어노테이션 검색 (@JsonProperty, @Expose 등)
3. 응답 래퍼 패턴 검색 (ResponseEntity, ApiResponse 등)
