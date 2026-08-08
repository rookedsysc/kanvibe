# Task Planner - Plan Template

Save the plan file to `.claude/plan/{domain}/{YYYY-MM}/{DD-slug}.plan.md`.

- **domain**: Inferred from task context (e.g., `auth`, `user`, `payment`, `infra`, `ui`)
- **YYYY-MM**: Current year-month (e.g., `2602` for Feb 2026)
- **DD-slug**: Day + kebab-case summary (e.g., `03-add-jwt-refresh`)

---

## Template

```markdown
# {Task Title}

## Business Goal
{이 작업을 수행하는 비즈니스 목적을 한국어로 서술}

## Scope
- **In Scope**: {포함 범위}
- **Out of Scope**: {제외 범위}

## Codebase Analysis Summary
{Phase 0에서 파악한 관련 코드 구조, 의존성, 패턴 요약}

### Relevant Files
| File | Role | Action |
|------|------|--------|
| {path} | {what it does} | Create / Modify / Reference |

### Conventions to Follow
| Convention | Source | Rule |
|-----------|--------|------|
| {naming, structure, etc.} | {where observed} | {specific rule to follow} |

## Architecture Decisions
| Decision | Choice | Rationale | Alternatives |
|----------|--------|-----------|--------------|
| {decision point} | {selected} | {reason} | {considered alternatives} |

## API Contracts (if applicable)

### {METHOD} {/path}
- Headers: {required headers}
- Request: `{schema}`
- Response: `{schema}`
- Note: {constraints}

## Data Models (if applicable)

### {Entity Name}
| Field | Type | Constraints |
|-------|------|-------------|
| {field} | {type} | {PK, FK, unique, etc.} |

## Implementation Todos

### Todo 1: {Title}
- **Priority**: {1 = independent, 2 = depends on tier 1, etc.}
- **Dependencies**: {todo IDs or "none"}
- **Goal**: {이 단계가 달성하려는 목표}
- **Work**:
  - {구체적인 작업 내용 1}
  - {구체적인 작업 내용 2}
  - {파일명, 함수명, 변수명 등 구체적으로 명시}
- **Convention Notes**: {이 단계에서 따라야 할 코드 컨벤션}
- **Verification**: {검증 방법 - 테스트, 빌드, lint 등}
- **Exit Criteria**: {완료 판단 기준}
- **Status**: pending

### Todo 2: {Title}
- **Priority**: ...
- **Dependencies**: ...
- **Goal**: ...
- **Work**: ...
- **Convention Notes**: ...
- **Verification**: ...
- **Exit Criteria**: ...
- **Status**: pending

(repeat for each todo)

## Verification Strategy
{전체 구현 완료 후 검증 방법}
- {test command or verification step}
- {integration/e2e check if applicable}

## Progress Tracking
- Total Todos: {N}
- Completed: 0
- Status: Planning complete

## Change Log
- {date}: Plan created
```

---

## Rules

- All section headers are required. Content within a section may be omitted only if genuinely not applicable.
- `API Contracts` and `Data Models` may be omitted for non-API tasks.
- `Architecture Decisions` is required for greenfield work; optional for small modifications.
- Every Todo must include Priority, Dependencies, Goal, Work, Convention Notes, Verification, and Exit Criteria.
- Priority tiers: 1 = independent (no dependencies), 2 = depends on tier 1, etc. Minimize dependencies for maximum parallel execution.
- Work descriptions must be specific: name files, functions, patterns. No vague descriptions.
- Business Goal and Scope descriptions should be in Korean.
- Todo status values: `pending`, `in_progress`, `completed`, `failed`
