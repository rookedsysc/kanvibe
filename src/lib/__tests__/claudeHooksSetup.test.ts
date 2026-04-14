import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { mkdtemp, mkdir, readFile, rm, writeFile } from "fs/promises";
import { join } from "path";
import { tmpdir } from "os";
import { setupClaudeHooks, getClaudeHooksStatus } from "../claudeHooksSetup";

describe("claudeHooksSetup", () => {
  let tempDir: string;

  beforeEach(async () => {
    tempDir = await mkdtemp(join(tmpdir(), "claude-hook-test-"));
  });

  afterEach(async () => {
    await rm(tempDir, { recursive: true, force: true });
  });

  it("토큰이 있으면 모든 hook 스크립트에 인증 헤더를 포함한다", async () => {
    // Given
    const repoPath = tempDir;

    // When
    await setupClaudeHooks(repoPath, "task-1", "http://192.168.0.10:9736", "auth-token");

    // Then
    const promptScript = await readFile(join(repoPath, ".claude", "hooks", "kanvibe-prompt-hook.sh"), "utf-8");
    const stopScript = await readFile(join(repoPath, ".claude", "hooks", "kanvibe-stop-hook.sh"), "utf-8");
    const questionScript = await readFile(join(repoPath, ".claude", "hooks", "kanvibe-question-hook.sh"), "utf-8");

    expect(promptScript).toContain('X-Kanvibe-Token: auth-token');
    expect(stopScript).toContain('X-Kanvibe-Token: auth-token');
    expect(questionScript).toContain('X-Kanvibe-Token: auth-token');

    const status = await getClaudeHooksStatus(repoPath);
    expect(status.installed).toBe(true);
  });

  it("브랜치 기반 legacy hook 스크립트도 설치됨으로 인식한다", async () => {
    // Given
    const repoPath = tempDir;
    const hooksDir = join(repoPath, ".claude", "hooks");
    await mkdir(hooksDir, { recursive: true });

    await writeFile(join(hooksDir, "kanvibe-prompt-hook.sh"), `#!/bin/bash\nPROJECT_NAME="kanvibe"\nBRANCH_NAME=$(git rev-parse --abbrev-ref HEAD 2>/dev/null)\ncurl -d "{\\"branchName\\": \\\"\${BRANCH_NAME}\\\", \\\"projectName\\": \\\"\${PROJECT_NAME}\\\", \\\"status\\": \\\"progress\\\"}"\n`, "utf-8");
    await writeFile(join(hooksDir, "kanvibe-stop-hook.sh"), `#!/bin/bash\nPROJECT_NAME="kanvibe"\nBRANCH_NAME=$(git rev-parse --abbrev-ref HEAD 2>/dev/null)\ncurl -d "{\\"branchName\\": \\\"\${BRANCH_NAME}\\\", \\\"projectName\\": \\\"\${PROJECT_NAME}\\\", \\\"status\\": \\\"review\\\"}"\n`, "utf-8");
    await writeFile(join(hooksDir, "kanvibe-question-hook.sh"), `#!/bin/bash\nPROJECT_NAME="kanvibe"\nBRANCH_NAME=$(git rev-parse --abbrev-ref HEAD 2>/dev/null)\ncurl -d "{\\"branchName\\": \\\"\${BRANCH_NAME}\\\", \\\"projectName\\": \\\"\${PROJECT_NAME}\\\", \\\"status\\": \\\"pending\\\"}"\n`, "utf-8");
    await writeFile(join(repoPath, ".claude", "settings.json"), JSON.stringify({
      hooks: {
        UserPromptSubmit: [{ hooks: [{ type: "command", command: '"$CLAUDE_PROJECT_DIR"/.claude/hooks/kanvibe-prompt-hook.sh', timeout: 10 }] }],
        Stop: [{ hooks: [{ type: "command", command: '"$CLAUDE_PROJECT_DIR"/.claude/hooks/kanvibe-stop-hook.sh', timeout: 10 }] }],
        PreToolUse: [{ matcher: "AskUserQuestion", hooks: [{ type: "command", command: '"$CLAUDE_PROJECT_DIR"/.claude/hooks/kanvibe-question-hook.sh', timeout: 10 }] }],
        PostToolUse: [{ matcher: "AskUserQuestion", hooks: [{ type: "command", command: '"$CLAUDE_PROJECT_DIR"/.claude/hooks/kanvibe-prompt-hook.sh', timeout: 10 }] }],
      },
    }), "utf-8");

    // When
    const status = await getClaudeHooksStatus(repoPath, "task-1");

    // Then
    expect(status.installed).toBe(true);
    expect(status.hasTaskIdBinding).toBe(true);
  });
});
