import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mkdtemp, readFile, rm, writeFile } from "fs/promises";
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

  it("모든 hook 스크립트는 인증 헤더 없이 상태만 전송한다", async () => {
    const repoPath = tempDir;

    await setupClaudeHooks(repoPath, "task-1", "http://192.168.0.10:9736");

    const promptScript = await readFile(join(repoPath, ".claude", "hooks", "kanvibe-prompt-hook.sh"), "utf-8");
    const stopScript = await readFile(join(repoPath, ".claude", "hooks", "kanvibe-stop-hook.sh"), "utf-8");
    const questionScript = await readFile(join(repoPath, ".claude", "hooks", "kanvibe-question-hook.sh"), "utf-8");

    expect(promptScript).not.toContain("X-Kanvibe-Token");
    expect(stopScript).not.toContain("X-Kanvibe-Token");
    expect(questionScript).not.toContain("X-Kanvibe-Token");

    const status = await getClaudeHooksStatus(repoPath);
    expect(status.installed).toBe(true);
  });

  it("stale Claude hook command entries are not treated as installed and are repaired on reinstall", async () => {
    const repoPath = tempDir;

    await setupClaudeHooks(repoPath, "task-1", "http://localhost:9736");

    await writeFile(
      join(repoPath, ".claude", "settings.json"),
      JSON.stringify({
        hooks: {
          UserPromptSubmit: [
            {
              hooks: [{ type: "command", command: '"/tmp/old-project/.claude/hooks/kanvibe-prompt-hook.sh"', timeout: 10 }],
            },
          ],
          PreToolUse: [
            {
              matcher: "AskUserQuestion",
              hooks: [{ type: "command", command: '"/tmp/old-project/.claude/hooks/kanvibe-question-hook.sh"', timeout: 10 }],
            },
          ],
          PostToolUse: [
            {
              matcher: "AskUserQuestion",
              hooks: [{ type: "command", command: '"/tmp/old-project/.claude/hooks/kanvibe-prompt-hook.sh"', timeout: 10 }],
            },
          ],
          Stop: [
            {
              hooks: [{ type: "command", command: '"/tmp/old-project/.claude/hooks/kanvibe-stop-hook.sh"', timeout: 10 }],
            },
          ],
        },
      }, null, 2) + "\n",
      "utf-8",
    );

    const staleStatus = await getClaudeHooksStatus(repoPath);
    expect(staleStatus.hasSettingsEntry).toBe(false);
    expect(staleStatus.installed).toBe(false);

    await setupClaudeHooks(repoPath, "task-1", "http://localhost:9736");

    const repairedSettings = JSON.parse(await readFile(join(repoPath, ".claude", "settings.json"), "utf-8"));
    expect(repairedSettings.hooks.UserPromptSubmit[0].hooks[0].command).toBe('"$CLAUDE_PROJECT_DIR"/.claude/hooks/kanvibe-prompt-hook.sh');
    expect(repairedSettings.hooks.PreToolUse[0].hooks[0].command).toBe('"$CLAUDE_PROJECT_DIR"/.claude/hooks/kanvibe-question-hook.sh');
    expect(repairedSettings.hooks.PostToolUse[0].hooks[0].command).toBe('"$CLAUDE_PROJECT_DIR"/.claude/hooks/kanvibe-prompt-hook.sh');
    expect(repairedSettings.hooks.Stop[0].hooks[0].command).toBe('"$CLAUDE_PROJECT_DIR"/.claude/hooks/kanvibe-stop-hook.sh');

    const repairedStatus = await getClaudeHooksStatus(repoPath);
    expect(repairedStatus.hasSettingsEntry).toBe(true);
    expect(repairedStatus.installed).toBe(true);
  });

  it("keeps Claude installed when hook server reachability fails but configuration is correct", async () => {
    const repoPath = tempDir;
    const mockFetch = vi.fn().mockRejectedValue(new Error("connection refused"));
    vi.stubGlobal("fetch", mockFetch);

    await setupClaudeHooks(repoPath, "task-1", "http://localhost:9736");
    const status = await getClaudeHooksStatus(repoPath, "task-1");

    expect(status.installed).toBe(true);
    expect(status.hasExpectedHookServerUrl).toBe(true);
    expect(status.hasReachableHookServer).toBe(false);

    vi.unstubAllGlobals();
  });
});
