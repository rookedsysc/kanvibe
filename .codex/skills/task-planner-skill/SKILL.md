---
name: task-planner-skill
description: Full-lifecycle implementation planner and executor. Analyzes codebase (Serena memory, CLAUDE.md, code conventions), clarifies requirements through iterative Q&A, generates detailed implementation plans to .claude/plan/, and auto-executes. This skill should be used when the user wants to plan and implement a feature, fix a bug with a plan, or needs structured task decomposition with execution. Trigger keywords include plan, implement, task plan, feature plan, 구현 계획, 작업 계획, 계획 세워줘, plan and execute, 플랜, 구현해줘.
---

# Task Planner

Full-lifecycle skill: Analyze → Clarify → Plan → Execute.

## Phases Overview

```
Phase 0: Codebase Analysis (auto)
Phase 1: Requirement Clarification (iterative, interactive)
Phase 2: Plan Generation (after approval → file output)
Phase 3: Auto-Execution (sequential, with verification)
```

---

## Phase 0: Codebase Analysis

Gather context before engaging the user. This phase is silent — no output to user.

### Steps

1. **Read Serena memories** — `list_memories` → read relevant memories
2. **Read project instructions** — CLAUDE.md, CLAUDE.local.md
3. **Scan codebase conventions** — Use Serena `get_symbols_overview`, `find_symbol`, `search_for_pattern` to understand:
   - Directory structure and naming patterns
   - Code style (indentation, imports, exports)
   - Existing patterns (error handling, logging, testing)
   - Tech stack and dependencies
4. **Record findings internally** — Keep a mental model of:
   - Relevant existing code that will be touched
   - Conventions to follow
   - Potential conflicts or constraints

### Context Budget Rule

- Do NOT read entire files. Use symbolic tools.
- Read only what is relevant to the user's request domain.
- If the request domain is unclear, defer deeper analysis to after Phase 1.

---

## Phase 1: Requirement Clarification

Transform the user's request into precise, actionable specification through iterative questioning.

### Step 1: Capture & Assess

Record the original requirement verbatim. Assess uncertainty level:

| Level | State | Action |
|-------|-------|--------|
| **LOW** | Clear | Apply defaults, record assumptions, proceed |
| **MEDIUM** | Partially ambiguous | Present 2-3 options via AskUserQuestion |
| **HIGH** | Very ambiguous | Block — must ask before proceeding |

### Uncertainty Triggers (→ MEDIUM/HIGH)

- Business logic decisions needed
- Security/authentication decisions
- Possible conflict with existing code
- Subjective requirements ("good", "fast", "pretty")
- Scope feels unlimited
- Multiple valid implementation approaches

### Step 2: Iterative Q&A

Use `AskUserQuestion` tool. Design questions with:
- **Specific over general**: Concrete details, not abstract preferences
- **Options over open-ended**: 2-4 choices (recognition > recall)
- **One concern per question**: No bundling
- **Neutral framing**: No bias toward any option

Continue until all ambiguities are resolved.

### Step 3: Present Proposal

Output structured proposal in chat (no files yet):

```markdown
# Implementation Proposal

## Interpreted Request
{summary of what user wants, in Korean}

## Candidate Approaches
- **Option A**: {approach + rationale}
- **Option B**: {approach + rationale}

## Recommended Approach
{selected option with reasoning}

## Proposed Scope
- **In Scope**: {features and deliverables}
- **Out of Scope**: {explicitly excluded}

## Architecture Decisions
| Decision | Choice | Rationale | Alternatives |
|----------|--------|-----------|--------------|
| {point}  | {pick} | {why}     | {others}     |

## Convention Compliance
{how this implementation follows existing codebase patterns}

## Open Questions
{remaining unknowns, if any}

## Next Step
→ Approve to generate plan, or provide feedback.
```

### Step 4: Iterate

Revise proposal based on user feedback. Repeat Steps 2-3 until explicit approval.

**Approval Trigger**: Clear positive feedback (e.g., "좋아", "진행", "승인", "go", "go ahead", "sounds good", "ㄱㄱ")

---

## Phase 2: Plan Generation

Triggered only after explicit approval. Load `references/plan-template.md` for format.

### Plan File Location

```
.claude/plan/{domain}/{YYYY-MM}/{DD-slug}.plan.md
```

- **domain**: Auto-inferred from task context (e.g., `auth`, `user`, `payment`, `infra`). Present to user for confirmation.
- **YYYY-MM**: Current year-month (e.g., `2602`)
- **DD-slug**: Day + kebab-case summary (e.g., `03-add-jwt-refresh`)

### Plan Content Requirements

Every plan must include:
1. **Business Goal** — Why this is being done (in Korean)
2. **Implementation Details** — Which files to create/modify, with specifics
3. **Convention Compliance** — How to follow existing patterns (naming, structure, error handling)
4. **Step-by-step Todos** — Each with:
   - Priority tier, dependencies, goal, detailed work description, verification criteria, exit criteria
   - Minimize dependencies for maximum parallel execution
5. **Verification Strategy** — How correctness is validated

### Plan Generation Steps

1. Create directory structure if needed
2. Generate plan file following `references/plan-template.md`
3. Report plan location to user
4. Summarize: total todos, key decisions, estimated scope

---

## Phase 3: Auto-Execution

After plan is saved, execute todos sequentially. Load `references/execution-protocol.md` for rules.

### Execution Flow

```
1. Build dependency graph from todo priorities
2. For each priority tier (1 → 2 → N):
   - Execute independent todos in parallel where possible (Task tool)
   - Verify each todo upon completion
   - Completing a todo unblocks dependent todos in next tier
3. Run final verification after all todos complete
```

### Execution Rules

- Respect dependency order. Todos with no dependencies (tier 1) execute first.
- Independent todos within the same tier run in parallel when feasible.
- Never execute a todo before its dependencies are completed.
- Follow convention compliance notes from the plan exactly.
- If a todo fails verification, attempt fix up to 2 times. On 3rd failure, stop and report to user.
- After all todos complete, run final verification and report summary.

### Error Recovery

| Situation | Action |
|-----------|--------|
| Test failure | Fix and retry (max 2 attempts) |
| Build failure | Fix and retry (max 2 attempts) |
| Unexpected conflict | Stop, report to user with context |
| Scope creep detected | Stop, ask user before expanding |

---

## Core Rules

1. **No assumptions without marking**: Use `Assumption:` prefix for any inferred decisions
2. **Convention-first**: Match existing codebase patterns over personal preference
3. **Minimal scope**: Implement only what's planned. No speculative additions.
4. **Measurable criteria**: Every todo has testable exit criteria
5. **Progressive context loading**: Load references only when their phase begins

## References

- Plan template: `references/plan-template.md` (load at Phase 2)
- Execution protocol: `references/execution-protocol.md` (load at Phase 3)
