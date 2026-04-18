import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { mkdtemp, readFile, rm } from "fs/promises";
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
    const repoPath = tempDir;

    await setupClaudeHooks(repoPath, "task-1", "http://192.168.0.10:9736", "auth-token");

    const promptScript = await readFile(join(repoPath, ".claude", "hooks", "kanvibe-prompt-hook.sh"), "utf-8");
    const stopScript = await readFile(join(repoPath, ".claude", "hooks", "kanvibe-stop-hook.sh"), "utf-8");
    const questionScript = await readFile(join(repoPath, ".claude", "hooks", "kanvibe-question-hook.sh"), "utf-8");

    expect(promptScript).toContain("X-Kanvibe-Token: auth-token");
    expect(stopScript).toContain("X-Kanvibe-Token: auth-token");
    expect(questionScript).toContain("X-Kanvibe-Token: auth-token");

    const status = await getClaudeHooksStatus(repoPath);
    expect(status.installed).toBe(true);
  });
});
