# Task Planner - Execution Protocol

## Overview

After plan approval and file generation, execute todos respecting dependency order. Independent todos within the same priority tier run in parallel when possible.

```
[Plan Approved] → Read Plan → Build Dependency Graph → Execute by Priority Tier → Final Verification → Report
```

---

## Pre-Execution

1. Read the plan file from `.claude/plan/{domain}/{YYYY-MM}/{DD-slug}.plan.md`
2. Verify plan has todos with status `pending`
3. Read `Conventions to Follow` section — internalize before writing any code

---

## Dependency Graph & Parallel Execution

### Priority Tiers

Each todo has a priority tier derived from its dependencies:
- **Tier 1**: No dependencies — execute first, all tier-1 todos can run in parallel
- **Tier 2**: Depends on tier-1 todos — execute after all tier-1 dependencies complete
- **Tier N**: Depends on tier N-1 — execute after all dependencies complete

### Execution Order

```
Tier 1: [Todo A, Todo B, Todo C]  → run in parallel (no dependencies)
Tier 2: [Todo D (→A), Todo E (→B)] → run in parallel after their dependencies complete
Tier 3: [Todo F (→D,E)]            → run after D and E complete
```

- Within the same tier, todos with no mutual dependencies run in parallel (use Task tool for parallel agent spawning)
- If a todo depends on another within the same tier, execute the dependency first
- Single-agent context: "parallel" means launching independent work via Task tool subagents where applicable. When subagents are not available, process tier todos sequentially but in dependency-safe order.

---

## Todo Execution Loop

For each todo (respecting dependency order):

### Step 1: Start Todo
- Verify all dependencies are `completed`. If not, skip and process next eligible todo.
- Update todo status: `pending` → `in_progress`
- Read the todo's Work and Convention Notes sections

### Step 2: Analyze Before Coding
- Use Serena symbolic tools to read relevant existing code
- Understand the current state of files listed in the todo
- Do NOT read entire files — use `find_symbol`, `get_symbols_overview`

### Step 3: Implement
- Execute the work described in the todo
- Follow Convention Notes strictly
- Prefer editing existing files over creating new ones
- Write minimal, focused code — no speculative additions

### Step 4: Verify
- Run verification steps specified in the todo
- Common verifications:
  - `build`: Ensure project compiles/builds
  - `lint`: Ensure no linting errors introduced
  - `test`: Run relevant test suite
  - `type-check`: Ensure type safety
- If verification fails: fix and retry (max 2 attempts per todo)

### Step 5: Complete
- Update todo status: `in_progress` → `completed`
- Update Progress Tracking section in plan file
- Add entry to Change Log
- Check if completing this todo unblocks any tier N+1 todos → process them next

---

## Error Handling

### Verification Failure (test/lint/build)
```
Attempt 1: Fix the specific error, re-run verification
Attempt 2: Fix with broader context, re-run verification
Attempt 3: STOP. Mark todo as `failed`. Report to user with:
  - What failed
  - What was attempted
  - Suggested next steps
```

### Unexpected Code Conflict
- STOP execution
- Report the conflict to user
- Wait for guidance before proceeding

### Scope Creep Detection
If during execution a todo requires work not described in the plan:
- STOP
- Report the discovered additional work
- Ask user: add to plan or skip?

---

## Post-Execution

After all todos are completed:

### Final Verification
1. Run full test suite (if applicable)
2. Run build/compile
3. Run lint
4. Verify all acceptance criteria from the plan

### Summary Report
Output to user:
```
## Execution Complete

### Results
- Total Todos: {N}
- Completed: {N}
- Failed: {N}

### Files Changed
| File | Action | Description |
|------|--------|-------------|
| {path} | Created/Modified | {what changed} |

### Verification Results
- Build: PASS/FAIL
- Lint: PASS/FAIL
- Tests: PASS/FAIL ({N} passed, {N} failed)

### Notes
{any observations, warnings, or follow-up suggestions}
```

### Update Plan File
- Update Progress Tracking: Status → `Execution complete`
- Add final Change Log entry with completion date

---

## Rules

1. Never execute a todo before its dependencies are completed
2. Never skip a todo — all must eventually execute
3. Never modify a todo during execution — if change is needed, stop and report
4. Always verify before marking complete
5. Always update the plan file after each todo
6. Follow Convention Notes from the plan — these override personal preference
7. If stuck for more than 2 retry cycles, stop and ask the user
