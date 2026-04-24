import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { mkdtemp, readFile, rm, writeFile, mkdir } from "fs/promises";
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
      const repoPath = tempDir;

      await setupCodexHooks(repoPath, "task-1", "http://localhost:3000");

      const status = await getCodexHooksStatus(repoPath);
      expect(status.hasNotifyHook).toBe(true);
    });

    it("should create config.toml with notify entry", async () => {
      const repoPath = tempDir;

      await setupCodexHooks(repoPath, "task-1", "http://localhost:3000");

      const status = await getCodexHooksStatus(repoPath);
      expect(status.hasConfigEntry).toBe(true);

      const hookContent = await readFile(join(repoPath, ".codex", "hooks", "kanvibe-notify-hook.sh"), "utf-8");
      expect(hookContent).toContain('TASK_ID="task-1"');
      expect(hookContent).toContain("taskId");
    });

    it("should mark as installed when both hook and config exist", async () => {
      const repoPath = tempDir;

      await setupCodexHooks(repoPath, "task-1", "http://localhost:3000");

      const status = await getCodexHooksStatus(repoPath);
      expect(status.installed).toBe(true);
    });

    it("should not add duplicate notify entry", async () => {
      const repoPath = tempDir;
      await setupCodexHooks(repoPath, "task-1", "http://localhost:3000");

      await setupCodexHooks(repoPath, "task-1", "http://localhost:3000");

      const status = await getCodexHooksStatus(repoPath);
      expect(status.installed).toBe(true);
    });

    it("should replace an existing notify entry on reinstall", async () => {
      const repoPath = tempDir;
      await mkdir(join(repoPath, ".codex"), { recursive: true });
      await writeFile(join(repoPath, ".codex", "config.toml"), 'model = "gpt-5"\nnotify = ["old-notify.sh"]\n', "utf-8");

      await setupCodexHooks(repoPath, "task-1", "http://localhost:3000");

      const configContent = await readFile(join(repoPath, ".codex", "config.toml"), "utf-8");
      expect(configContent).toContain('notify = [".codex/hooks/kanvibe-notify-hook.sh"]');
      expect(configContent).not.toContain('notify = ["old-notify.sh"]');
      expect(configContent.match(/^notify\s*=/gm)).toHaveLength(1);
    });
  });

  describe("getCodexHooksStatus", () => {
    it("should return not installed when no files exist", async () => {
      const repoPath = tempDir;

      const status = await getCodexHooksStatus(repoPath);

      expect(status.installed).toBe(false);
      expect(status.hasNotifyHook).toBe(false);
      expect(status.hasConfigEntry).toBe(false);
    });

    it("should detect installed status correctly", async () => {
      const repoPath = tempDir;
      await setupCodexHooks(repoPath, "task-1", "http://localhost:3000");

      const status = await getCodexHooksStatus(repoPath);

      expect(status).toEqual({
        installed: true,
        hasNotifyHook: true,
        hasConfigEntry: true,
        hasTaskIdBinding: true,
        hasReviewStatus: true,
        hasAgentTurnCompleteFilter: true,
        hasExpectedHookServerUrl: true,
        hasReachableHookServer: true,
        boundTaskId: "task-1",
        configuredHookServerUrl: "http://localhost:3000",
        expectedHookServerUrl: null,
      });
    });
  });
});
