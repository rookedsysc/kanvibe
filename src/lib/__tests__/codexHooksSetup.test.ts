import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { mkdtemp, readFile, rm, writeFile, mkdir } from "fs/promises";
import { join } from "path";
import { tmpdir } from "os";
import {
  setupCodexHooks,
  getCodexHooksStatus,
  upsertCodexConfigToml,
  upsertCodexHooksJson,
} from "../codexHooksSetup";

describe("codexHooksSetup", () => {
  let tempDir: string;

  beforeEach(async () => {
    tempDir = await mkdtemp(join(tmpdir(), "codex-test-"));
  });

  afterEach(async () => {
    await rm(tempDir, { recursive: true, force: true });
  });

  describe("upsertCodexConfigToml", () => {
    it("should enable codex_hooks under the features table", () => {
      const content = 'model = "gpt-5"\n[features]\nfast_mode = true\n';

      const updated = upsertCodexConfigToml(content);

      expect(updated).toContain("[features]");
      expect(updated).toContain("fast_mode = true");
      expect(updated).toContain("codex_hooks = true");
    });

    it("should remove the legacy kanvibe notify entry", () => {
      const content = 'model = "gpt-5"\nnotify = [".codex/hooks/kanvibe-notify-hook.sh"]\n';

      const updated = upsertCodexConfigToml(content);

      expect(updated).not.toContain('notify = [".codex/hooks/kanvibe-notify-hook.sh"]');
      expect(updated).toContain("[features]");
      expect(updated).toContain("codex_hooks = true");
    });
  });

  describe("upsertCodexHooksJson", () => {
    it("should register the current codex lifecycle hooks", () => {
      const updated = upsertCodexHooksJson("");

      expect(updated).toContain('"UserPromptSubmit"');
      expect(updated).toContain('"PermissionRequest"');
      expect(updated).toContain('"PreToolUse"');
      expect(updated).toContain('"Stop"');
      expect(updated).toContain('kanvibe-prompt-hook.sh');
      expect(updated).toContain('kanvibe-permission-hook.sh');
      expect(updated).toContain('kanvibe-pre-tool-hook.sh');
      expect(updated).toContain('kanvibe-stop-hook.sh');
    });
  });

  describe("setupCodexHooks - file operations", () => {
    it("should create all codex hook script files", async () => {
      const repoPath = tempDir;

      await setupCodexHooks(repoPath, "task-1", "http://localhost:3000");

      const status = await getCodexHooksStatus(repoPath);
      expect(status.hasPromptHook).toBe(true);
      expect(status.hasPermissionHook).toBe(true);
      expect(status.hasPreToolHook).toBe(true);
      expect(status.hasStopHook).toBe(true);
    });

    it("should create config.toml and hooks.json entries", async () => {
      const repoPath = tempDir;

      await setupCodexHooks(repoPath, "task-1", "http://localhost:3000");

      const status = await getCodexHooksStatus(repoPath);
      expect(status.hasConfigEntry).toBe(true);
      expect(status.hasHooksFile).toBe(true);
      expect(status.hasHookEntries).toBe(true);

      const promptHookContent = await readFile(join(repoPath, ".codex", "hooks", "kanvibe-prompt-hook.sh"), "utf-8");
      expect(promptHookContent).toContain('TASK_ID="task-1"');
      expect(promptHookContent).toContain("taskId");

      const configContent = await readFile(join(repoPath, ".codex", "config.toml"), "utf-8");
      expect(configContent).toContain("[features]");
      expect(configContent).toContain("codex_hooks = true");

      const hooksContent = await readFile(join(repoPath, ".codex", "hooks.json"), "utf-8");
      expect(hooksContent).toContain('"UserPromptSubmit"');
      expect(hooksContent).toContain('"PermissionRequest"');
      expect(hooksContent).toContain('"PreToolUse"');
      expect(hooksContent).toContain('"Stop"');
    });

    it("should mark as installed when all hook files and config exist", async () => {
      const repoPath = tempDir;

      await setupCodexHooks(repoPath, "task-1", "http://localhost:3000");

      const status = await getCodexHooksStatus(repoPath);
      expect(status.installed).toBe(true);
    });

    it("should not duplicate hook registrations on reinstall", async () => {
      const repoPath = tempDir;
      await setupCodexHooks(repoPath, "task-1", "http://localhost:3000");

      await setupCodexHooks(repoPath, "task-1", "http://localhost:3000");

      const hooksContent = await readFile(join(repoPath, ".codex", "hooks.json"), "utf-8");
      expect(hooksContent.match(/kanvibe-prompt-hook\.sh/g)?.length).toBe(1);
      expect(hooksContent.match(/kanvibe-permission-hook\.sh/g)?.length).toBe(1);
      expect(hooksContent.match(/kanvibe-pre-tool-hook\.sh/g)?.length).toBe(1);
      expect(hooksContent.match(/kanvibe-stop-hook\.sh/g)?.length).toBe(1);
    });

    it("should replace legacy notify config on reinstall", async () => {
      const repoPath = tempDir;
      await mkdir(join(repoPath, ".codex"), { recursive: true });
      await writeFile(
        join(repoPath, ".codex", "config.toml"),
        'model = "gpt-5"\nnotify = [".codex/hooks/kanvibe-notify-hook.sh"]\n',
        "utf-8",
      );

      await setupCodexHooks(repoPath, "task-1", "http://localhost:3000");

      const configContent = await readFile(join(repoPath, ".codex", "config.toml"), "utf-8");
      expect(configContent).not.toContain('notify = [".codex/hooks/kanvibe-notify-hook.sh"]');
      expect(configContent).toContain("[features]");
      expect(configContent).toContain("codex_hooks = true");
    });
  });

  describe("getCodexHooksStatus", () => {
    it("should return not installed when no files exist", async () => {
      const repoPath = tempDir;

      const status = await getCodexHooksStatus(repoPath);

      expect(status.installed).toBe(false);
      expect(status.hasPromptHook).toBe(false);
      expect(status.hasPermissionHook).toBe(false);
      expect(status.hasPreToolHook).toBe(false);
      expect(status.hasStopHook).toBe(false);
      expect(status.hasHooksFile).toBe(false);
      expect(status.hasConfigEntry).toBe(false);
    });

    it("should detect installed status correctly", async () => {
      const repoPath = tempDir;
      await setupCodexHooks(repoPath, "task-1", "http://localhost:3000");

      const status = await getCodexHooksStatus(repoPath);

      expect(status).toEqual({
        installed: true,
        hasPromptHook: true,
        hasPermissionHook: true,
        hasPreToolHook: true,
        hasStopHook: true,
        hasHooksFile: true,
        hasHookEntries: true,
        hasConfigEntry: true,
        hasTaskIdBinding: true,
        hasStatusMappings: true,
        hasExpectedHookServerUrl: true,
        hasReachableHookServer: true,
        boundTaskId: "task-1",
        configuredHookServerUrl: "http://localhost:3000",
        expectedHookServerUrl: null,
      });
    });
  });
});
