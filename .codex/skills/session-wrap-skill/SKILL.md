---
name: session-wrap-skill
description: Capture and document unstaged git changes using Serena memory. Use when the user asks to preserve uncommitted work or wants to save session context. Triggers include (1) End-of-session saves before closing or switching projects, (2) Mid-session checkpoints during complex tasks, (3) Preserving work-in-progress discoveries, (4) Session handoff preparation, (5) Storing uncommitted work for cross-session continuity. Keywords "wrap unstaged", "save changes", "document diff", "checkpoint", "preserve work", "save to serena".
---

# Session Wrap Skill (Serena Memory)

Capture unstaged git changes and store them as Serena memory with meaningful names.

## Quick Start

```bash
# Interactive: Prompts for memory name
/session-wrap-skill

# With specific memory name
/session-wrap-skill auth-refactoring
```

## Behavioral Flow

1. **Detect**: Check for unstaged changes via git status
2. **Capture**: Extract diff content using capture script
3. **Analyze**: Identify domain and scope from changed files
4. **Name**: Generate semantic memory name or prompt user
5. **Persist**: Save to Serena memory with structured format
6. **Confirm**: Validate save and provide retrieval instructions

Key behaviors:
- Mandatory Serena MCP integration for memory operations and persistence
- Intelligent domain identification from changed file paths and patterns
- Automatic semantic naming based on diff analysis
- User interaction for ambiguous or multi-domain changes
- Memory conflict detection with merge/overwrite strategies
- Performance-optimized saves with <2s for standard operations

## Workflow

### Step 1: Check Git Status

```bash
git status --short
git diff --stat
```

If no unstaged changes exist, inform user and exit.

### Step 2: Capture Changes

Run capture script to get structured diff data:

```bash
uv run scripts/capture_unstaged.py
```

**Script output:**
- Branch name
- Unstaged/untracked files
- Diff statistics
- Full diff content

### Step 3: Generate Memory Name

If memory name not provided as argument, generate meaningful name from diff analysis:

**Pattern:** `{feature-description}.md`

**Examples:**
- `auth-refactoring.md`
- `api-error-handling.md`
- `ui-button-component.md`

**Generation logic:**
1. Analyze changed files (identify domain: auth, api, ui, etc.)
2. Check diff stat for scope (refactor, feature, fix)
3. Combine into descriptive name

**If ambiguous:** Ask user for memory name using AskUserQuestion.

### Step 4: Store in Serena Memory

```python
mcp__oraios_serena__write_memory(
    memory_file_name=f"{feature-description}.md",
    content=captured_content
)
```

**Memory content format:**
```markdown
# Session Changes - {feature-description}

Branch: `{branch-name}`

## Summary
- Changed files: X
- Untracked files: Y

## Diff Statistics
{diff-stat}

## Files Changed
{file-list}

## Full Diff
{diff-content}
```

### Step 5: Confirm Completion

```
✅ Session changes saved to Serena memory

Memory: {feature-description}.md
Location: .serena/memories/

To review: Read memory or use Serena's read_memory tool
```

## MCP Integration

- **Serena MCP**: Core requirement for memory management and persistence
- **Memory Operations**: write_memory for persistence, read_memory for validation, list_memories for conflict detection
- **Discovery Analysis**: Automatic domain/scope detection from git diff patterns
- **Performance Critical**: <200ms for memory writes, <1s for checkpoint creation, <2s end-to-end

## Tool Coordination

- **write_memory**: Core memory persistence with UTF-8 content storage
- **read_memory**: Existing memory verification and update coordination
- **list_memories**: Memory inventory and naming conflict prevention
- **Bash (git status/diff)**: Change detection and diff capture
- **Bash (uv run script)**: Structured diff extraction via capture_unstaged.py
- **AskUserQuestion**: Memory name disambiguation when domain is ambiguous

## Key Patterns

- **Change Capture**: Git status → diff extraction → structured content
- **Semantic Naming**: Domain detection → scope analysis → descriptive name
- **Memory Persistence**: Conflict check → content format → Serena write
- **User Interaction**: Ambiguity detection → option presentation → name confirmation
- **Incremental Updates**: Existing memory check → collision handling → merge strategy

## Memory Naming Best Practices

**Good names** (semantic, searchable):
- `auth-refactoring.md`
- `payment-integration.md`
- `performance-optimization.md`

**Bad names** (not semantic):
- ❌ `2026-01-29.md` (timestamp)
- ❌ `changes.md` (too generic)
- ❌ `work-123.md` (meaningless number)

**Why semantic names:**
- Easy to find relevant memories
- Self-documenting content
- Better for AI retrieval
- Supports knowledge reuse

## Memory Name Generation Examples

| Changed Files | Suggested Name |
|---------------|----------------|
| `auth/*.py` | `auth-implementation.md` |
| `api/endpoints/*.ts` | `api-endpoints.md` |
| `components/Button.tsx` | `ui-button-component.md` |
| Multiple domains | Ask user for clarification |

## User Interaction (No --name Flag)

When memory name is ambiguous, use AskUserQuestion:

```python
AskUserQuestion(
    questions=[{
        "question": f"Changes detected in {domains}. What should this memory be named?",
        "header": "Memory Name",
        "multiSelect": false,
        "options": [
            {"label": suggested_name_1, "description": "Based on primary changes"},
            {"label": suggested_name_2, "description": "Alternative interpretation"},
            {"label": "Custom name", "description": "You'll provide the name"}
        ]
    }]
)
```

If "Custom name" selected, prompt for manual input.

## Boundaries

**Will:**
- Capture unstaged changes using git diff and store in Serena memory
- Generate semantic memory names based on changed files and scope
- Prompt for user input when domain is ambiguous
- Handle memory name collisions with merge/overwrite options
- Validate memory write success before confirming completion

**Will Not:**
- Operate without Serena MCP availability and proper initialization
- Save changes without proper diff analysis and content validation
- Override existing memories without explicit user confirmation
- Create memories for empty or trivial diffs with no meaningful changes
- Commit or push changes to git (use commit-skill for that)

## Best Practices

**When to Use:**
- ✅ After completing feature work but before committing
- ✅ When discovering important patterns during implementation
- ✅ Before switching contexts or ending work session
- ✅ After resolving complex bugs with reusable insights
- ✅ When documenting work-in-progress for session handoff

**Save Frequency:**
- Quick saves: Every 30-45 minutes for long sessions
- Checkpoint saves: After major milestones or before risky changes
- Discovery saves: Immediately when insights emerge
- End-of-session saves: Always before closing or context switching

**Memory Hygiene:**
- Review and consolidate memories periodically (monthly cleanup)
- Use clear, consistent naming for easy retrieval
- Include enough context for future understanding
- Archive outdated memories rather than deleting (preserve history)

## Performance

- Memory write: <200ms for standard operations
- Checkpoint creation: <1s for full session capture
- Git diff extraction: <500ms for typical changes
- Total operation: <2s end-to-end for interactive flow

## Error Handling

**No unstaged changes:**
```
ℹ No unstaged changes detected.
Would you like to wrap staged changes instead?
```

**Serena not initialized:**
```
⚠ Serena MCP not available.

To enable Serena:
1. Run /serena-init
2. Complete onboarding
```

**Memory name collision:**
```
⚠ Memory {name}.md already exists.

Options:
1. Overwrite existing memory
2. Append suffix: {name}-v2
3. Choose different name
```

**Performance Issues:**
- Large diff (>10k lines): Warn user about memory size
- Script execution timeout: Check Python environment and dependencies
- Slow git operations: Consider using --stat only for large repos
- Memory write failures: Verify Serena MCP connection is active and responsive
- Permission errors: Check memory storage permissions and available space

## Script Dependencies

**Required:** `scripts/capture_unstaged.py`

```bash
uv run scripts/capture_unstaged.py
```

Outputs markdown-formatted diff data to stdout.

## Workflow Integration with commit-skill

**Recommended sequence:**

```bash
# Step 1: Save unstaged changes to Serena memory (knowledge preservation)
/session-wrap-skill auth-refactoring

# Step 2: Commit the changes
/commit-skill
```

**Why this sequence:**
1. **Knowledge preservation first** - Serena memory captures the context and rationale
2. **Commit second** - Git history records the actual changes
3. **Cross-reference** - Future sessions can reference both Serena memory and git commits

**Example workflow:**
```bash
# Working on authentication refactoring...
# Made significant changes but not ready to commit yet

# Save work-in-progress to Serena
/session-wrap-skill auth-refactoring-wip

# Continue working...
# Ready to commit

# Save final state to Serena
/session-wrap-skill auth-refactoring-final

# Create conventional commit
/commit-skill --only-commit
# → Generates: "refactor(auth): implement JWT token validation"
```

**Benefits:**
- Serena memory preserves **why** decisions were made
- Git commits record **what** changed
- Both together provide complete project history

## Integration Notes

**Works with commit-skill:**
- This skill captures unstaged changes to Serena memory
- Then use commit-skill to create conventional commits
- Independent operation - no dependencies

**Cross-session retrieval:**
```python
# Later sessions can read these memories
mcp__oraios_serena__read_memory("auth-implementation.md")
```
