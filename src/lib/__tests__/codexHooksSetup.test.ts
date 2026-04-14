import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { mkdtemp, mkdir, readFile, rm, writeFile } from "fs/promises";
import { join } from "path";
import { tmpdir } from "os";
import { setupCodexHooks, getCodexHooksStatus } from "../codexHooksSetup";

describe("codexHooksSetup", () => {
  let tempDir: string;

  beforeEach(async () => {
    tempDir = await mkdtemp(join(tmpdir(), "codex-test-"));
  });

  afterEach(async () => {
    await rm(tempDir, { recursive: true, force: true });
  });

  describe("setupCodexHooks - file operations", () => {
    it("should create hook script file", async () => {
      // Given
      const repoPath = tempDir;

      // When
      await setupCodexHooks(repoPath, "task-1", "http://localhost:3000");

      // Then
      const status = await getCodexHooksStatus(repoPath);
      expect(status.hasNotifyHook).toBe(true);
    });

    it("should create config.toml with notify entry", async () => {
      // Given
      const repoPath = tempDir;

      // When
      await setupCodexHooks(repoPath, "task-1", "http://localhost:3000");

      // Then
      const status = await getCodexHooksStatus(repoPath);
      expect(status.hasConfigEntry).toBe(true);

      const hookContent = await readFile(join(repoPath, ".codex", "hooks", "kanvibe-notify-hook.sh"), "utf-8");
      expect(hookContent).toContain("TASK_ID=\"task-1\"");
      expect(hookContent).toContain("taskId");
    });

    it("should mark as installed when both hook and config exist", async () => {
      // Given
      const repoPath = tempDir;

      // When
      await setupCodexHooks(repoPath, "task-1", "http://localhost:3000");

      // Then
      const status = await getCodexHooksStatus(repoPath);
      expect(status.installed).toBe(true);
    });

    it("should not add duplicate notify entry", async () => {
      // Given
      const repoPath = tempDir;
      await setupCodexHooks(repoPath, "task-1", "http://localhost:3000");

      // When - setup again
      await setupCodexHooks(repoPath, "task-1", "http://localhost:3000");

      // Then - should still be installed
      const status = await getCodexHooksStatus(repoPath);
      expect(status.installed).toBe(true);
    });
  });

  describe("getCodexHooksStatus", () => {
    it("should return not installed when no files exist", async () => {
      // Given
      const repoPath = tempDir;

      // When
      const status = await getCodexHooksStatus(repoPath);

      // Then
      expect(status.installed).toBe(false);
      expect(status.hasNotifyHook).toBe(false);
      expect(status.hasConfigEntry).toBe(false);
    });

    it("should detect installed status correctly", async () => {
      // Given
      const repoPath = tempDir;
      await setupCodexHooks(repoPath, "task-1", "http://localhost:3000");

      // When
      const status = await getCodexHooksStatus(repoPath);

      // Then
      expect(status).toEqual({
        installed: true,
        hasNotifyHook: true,
        hasConfigEntry: true,
        hasTaskIdBinding: true,
        hasReviewStatus: true,
        hasAgentTurnCompleteFilter: true,
      });
    });

    it("should treat legacy branch-targeted notify script as installed", async () => {
      // Given
      const repoPath = tempDir;
      const hooksDir = join(repoPath, ".codex", "hooks");
      await mkdir(hooksDir, { recursive: true });
      await writeFile(join(hooksDir, "kanvibe-notify-hook.sh"), `#!/bin/bash\nPROJECT_NAME="kanvibe"\nJSON_PAYLOAD="$1"\nEVENT_TYPE=$(echo "$JSON_PAYLOAD" | grep -o '"type":"[^"]*"' | head -1 | cut -d'"' -f4)\nif [ "$EVENT_TYPE" != "agent-turn-complete" ]; then\n  exit 0\nfi\nBRANCH_NAME=$(git rev-parse --abbrev-ref HEAD 2>/dev/null)\ncurl -d "{\\"branchName\\": \\\"\${BRANCH_NAME}\\\", \\\"projectName\\": \\\"\${PROJECT_NAME}\\\", \\\"status\\": \\\"review\\\"}"\n`, "utf-8");
      await writeFile(join(repoPath, ".codex", "config.toml"), 'notify = [".codex/hooks/kanvibe-notify-hook.sh"]\n', "utf-8");

      // When
      const status = await getCodexHooksStatus(repoPath, "task-1");

      // Then
      expect(status.installed).toBe(true);
      expect(status.hasTaskIdBinding).toBe(true);
    });
  });
});
