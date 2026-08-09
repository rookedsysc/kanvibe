# Mermaid Data Flow Diagram Templates

데이터 흐름 시각화를 위한 Mermaid 다이어그램 템플릿 모음.
MDX 문법 호환성을 위해 `@.claude/core/MDX.md` 규칙을 반드시 준수할 것.

---

## DB Column → Entity Field 매핑 다이어그램

DB 테이블의 컬럼과 코드 엔티티 필드 간 이름 차이를 시각화한다.

```mermaid
flowchart LR
    subgraph DB["DB Table: user_accounts"]
        db_id["id (BIGINT, PK)"]
        db_email["email_address (VARCHAR)"]
        db_created["created_at (TIMESTAMP)"]
        db_status["account_status (VARCHAR)"]
    end

    subgraph Entity["Entity: UserAccount"]
        e_id["id: Long"]
        e_email["emailAddress: String"]
        e_created["createdAt: LocalDateTime"]
        e_status["accountStatus: AccountStatus"]
    end

    db_id -->|"@Id"| e_id
    db_email -->|"@Column(name)"| e_email
    db_created -->|"@Column(name)"| e_created
    db_status -->|"@Enumerated"| e_status
```

---

## Entity → DTO 변환 흐름

Entity에서 DTO로 데이터가 변환되는 과정과 필드 매핑을 시각화한다.

```mermaid
flowchart LR
    subgraph Entity["Entity: UserAccount"]
        e_id["id: Long"]
        e_email["emailAddress: String"]
        e_created["createdAt: LocalDateTime"]
        e_status["accountStatus: AccountStatus"]
        e_password["password: String"]
        e_orders["orders: List\<Order\>"]
    end

    subgraph DTO["UserResponseDto"]
        d_id["userId: Long"]
        d_email["email: String"]
        d_created["joinDate: String"]
        d_status["status: String"]
        d_count["orderCount: int"]
    end

    e_id -->|"직접 매핑"| d_id
    e_email -->|"필드명 변경"| d_email
    e_created -->|"포맷 변환"| d_created
    e_status -->|"enum → string"| d_status
    e_orders -->|"size() 계산"| d_count
    e_password -.->|"❌ 제외"| excluded["응답에서 제외"]

    style excluded fill:#ffcccc,stroke:#cc0000
```

---

## 전체 데이터 흐름 (DB → Entity → DTO → Response)

하나의 API 요청에서 데이터가 DB부터 최종 응답까지 변환되는 전체 과정.

```mermaid
flowchart TD
    subgraph DB["Database"]
        t1["user_accounts"]
        t2["orders"]
        t3["order_items"]
    end

    subgraph Repository["Repository Layer"]
        r1["UserRepository"]
        r2["OrderRepository"]
    end

    subgraph Entity["Entity Layer"]
        e1["UserAccount"]
        e2["Order"]
        e3["OrderItem"]
    end

    subgraph Service["Service Layer"]
        s1["UserService.getUserDetail()"]
    end

    subgraph DTO["DTO Layer"]
        d1["UserDetailResponse"]
        d2["OrderSummaryDto"]
    end

    subgraph API["API Response"]
        resp["GET /api/users/:id"]
    end

    t1 --> r1
    t2 --> r2
    t3 --> r2
    r1 --> e1
    r2 --> e2
    r2 --> e3
    e1 --> s1
    e2 --> s1
    s1 --> d1
    e2 -->|"toSummary()"| d2
    d1 --> resp
    d2 -->|"nested in response"| resp
```

---

## ER 다이어그램 + 컬럼 매핑 주석

테이블 간 관계와 함께 코드에서의 필드명을 주석으로 병기한다.

```mermaid
erDiagram
    user_accounts {
        BIGINT id PK "UserAccount.id"
        VARCHAR email_address "UserAccount.emailAddress"
        TIMESTAMP created_at "UserAccount.createdAt"
        VARCHAR account_status "UserAccount.accountStatus (Enum)"
    }

    orders {
        BIGINT id PK "Order.id"
        BIGINT user_id FK "Order.user (ManyToOne)"
        DECIMAL total_amount "Order.totalAmount"
        VARCHAR order_status "Order.status"
        TIMESTAMP ordered_at "Order.orderedAt"
    }

    order_items {
        BIGINT id PK "OrderItem.id"
        BIGINT order_id FK "OrderItem.order (ManyToOne)"
        BIGINT product_id FK "OrderItem.productId"
        INT quantity "OrderItem.quantity"
        DECIMAL unit_price "OrderItem.unitPrice"
    }

    user_accounts ||--o{ orders : "1:N"
    orders ||--o{ order_items : "1:N"
```

---

## 필드 매핑 테이블 (다이어그램 보조)

다이어그램과 함께 상세한 필드 매핑을 테이블로 정리한다.

```markdown
### user_accounts → UserAccount → UserResponseDto

| DB Column | Type | Entity Field | Entity Type | DTO Field | DTO Type | 변환 방식 |
|-----------|------|-------------|-------------|-----------|----------|----------|
| id | BIGINT | id | Long | userId | Long | 직접 매핑 |
| email_address | VARCHAR | emailAddress | String | email | String | 필드명 변경 |
| created_at | TIMESTAMP | createdAt | LocalDateTime | joinDate | String | 포맷 변환 (yyyy-MM-dd) |
| account_status | VARCHAR | accountStatus | AccountStatus | status | String | enum.name() |
| password_hash | VARCHAR | password | String | - | - | 응답 제외 |
```

---

## 관계 매핑 시각화

N:1, 1:N, N:M 관계에서 데이터가 어떻게 변환되는지 보여준다.

```mermaid
flowchart LR
    subgraph DB["DB Tables"]
        users["user_accounts<br/>(user_id)"]
        orders["orders<br/>(user_id FK)"]
        tags["tags"]
        user_tags["user_tags<br/>(user_id FK, tag_id FK)"]
    end

    subgraph Code["Entity Relations"]
        u["User"]
        o["Order"]
        t["Tag"]
    end

    subgraph DTO["DTO Transform"]
        ud["UserDetailDto"]
        od["OrderSummaryDto"]
        td["tagNames: List\<String\>"]
    end

    users -->|"1:N"| orders
    users -->|"N:M via user_tags"| tags
    u -->|"@OneToMany"| o
    u -->|"@ManyToMany"| t
    o -->|"toSummary()"| od
    t -->|"map to name"| td
    u --> ud
    od -->|"nested list"| ud
    td -->|"string list"| ud
```

---

## 다이어그램 작성 규칙

- 노드 레이블에 특수문자 포함 시 반드시 따옴표로 감싸기: `["Promise<Data>"]`
- 줄바꿈: `<br/>`, 이탤릭: `<i>`, 볼드: `<b>`
- 서브그래프 이름에 공백 포함 시: `subgraph "서브그래프 이름"`
- 제외/에러 노드는 붉은색 스타일: `style node fill:#ffcccc,stroke:#cc0000`
- 현재 분석 대상은 초록색 스타일: `style node fill:#d4edda,stroke:#28a745,stroke-width:3px`
- 다이어그램당 노드 수 20개 이내로 유지
- 테스트: https://mermaid.live
