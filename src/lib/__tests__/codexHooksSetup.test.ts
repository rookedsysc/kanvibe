import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { mkdtemp, readFile, rm, mkdir, writeFile } from "fs/promises";
import { join } from "path";
import { tmpdir } from "os";
import { execSync } from "child_process";
import { setupCodexHooks, getCodexHooksStatus } from "../codexHooksSetup";

describe("codexHooksSetup", () => {
  let tempDir: string;

  beforeEach(async () => {
    tempDir = await mkdtemp(join(tmpdir(), "codex-test-"));
    execSync("git init", { cwd: tempDir, stdio: "ignore" });
  });

  afterEach(async () => {
    await rm(tempDir, { recursive: true, force: true });
  });

  describe("setupCodexHooks - file operations", () => {
    it("should create prompt and stop hook script files", async () => {
      const repoPath = tempDir;

      await setupCodexHooks(repoPath, "task-1", "http://localhost:3000");

      const status = await getCodexHooksStatus(repoPath);
      expect(status.hasPromptHook).toBe(true);
      expect(status.hasStopHook).toBe(true);
    });

    it("should create hooks.json and config.toml with Codex hooks entries", async () => {
      const repoPath = tempDir;

      await setupCodexHooks(repoPath, "task-1", "http://localhost:3000");

      const status = await getCodexHooksStatus(repoPath);
      expect(status.hasHooksJsonEntry).toBe(true);
      expect(status.hasFeatureFlag).toBe(true);

      const promptHookContent = await readFile(join(repoPath, ".codex", "hooks", "kanvibe-prompt-hook.sh"), "utf-8");
      const stopHookContent = await readFile(join(repoPath, ".codex", "hooks", "kanvibe-stop-hook.sh"), "utf-8");
      const hooksJsonContent = await readFile(join(repoPath, ".codex", "hooks.json"), "utf-8");
      const configContent = await readFile(join(repoPath, ".codex", "config.toml"), "utf-8");

      expect(promptHookContent).toContain('TASK_ID="task-1"');
      expect(promptHookContent).toContain('\\"status\\": \\"progress\\"');
      expect(stopHookContent).toContain('\\"status\\": \\"review\\"');
      expect(hooksJsonContent).toContain("UserPromptSubmit");
      expect(hooksJsonContent).toContain("Stop");
      expect(hooksJsonContent).toContain("$(git rev-parse --show-toplevel)/.codex/hooks/kanvibe-prompt-hook.sh");
      expect(configContent).toContain("[features]");
      expect(configContent).toContain("codex_hooks = true");
    });

    it("should mark as installed when scripts, hooks.json, and feature flag exist", async () => {
      const repoPath = tempDir;

      await setupCodexHooks(repoPath, "task-1", "http://localhost:3000");

      const status = await getCodexHooksStatus(repoPath);
      expect(status.installed).toBe(true);
    });

    it("should not add duplicate hooks.json entries", async () => {
      const repoPath = tempDir;
      await setupCodexHooks(repoPath, "task-1", "http://localhost:3000");

      await setupCodexHooks(repoPath, "task-1", "http://localhost:3000");

      const hooksJson = JSON.parse(await readFile(join(repoPath, ".codex", "hooks.json"), "utf-8"));
      expect(hooksJson.hooks.UserPromptSubmit).toHaveLength(1);
      expect(hooksJson.hooks.Stop).toHaveLength(1);
      expect(await getCodexHooksStatus(repoPath)).toMatchObject({ installed: true });
    });

    it("should preserve existing config while enabling codex hooks", async () => {
      const repoPath = tempDir;
      await mkdir(join(repoPath, ".codex"), { recursive: true });
      await writeFile(
        join(repoPath, ".codex", "config.toml"),
        "model = \"gpt-5.4\"\n\n[features]\ncodex_hooks = false\n",
        "utf-8",
      );

      await setupCodexHooks(repoPath, "task-1", "http://localhost:3000");

      const configContent = await readFile(join(repoPath, ".codex", "config.toml"), "utf-8");
      expect(configContent).toContain('model = "gpt-5.4"');
      expect(configContent).toContain("[features]");
      expect(configContent).toContain("codex_hooks = true");
      expect(configContent).not.toContain("codex_hooks = false");
    });

    it("should remove legacy KanVibe notify config to avoid duplicate review updates", async () => {
      const repoPath = tempDir;
      await mkdir(join(repoPath, ".codex"), { recursive: true });
      await writeFile(
        join(repoPath, ".codex", "config.toml"),
        'notify = [".codex/hooks/kanvibe-notify-hook.sh"]\n',
        "utf-8",
      );

      await setupCodexHooks(repoPath, "task-1", "http://localhost:3000");

      const configContent = await readFile(join(repoPath, ".codex", "config.toml"), "utf-8");
      expect(configContent).not.toContain("kanvibe-notify-hook.sh");
      expect(configContent).toContain("codex_hooks = true");
    });
  });

  describe("getCodexHooksStatus", () => {
    it("should return not installed when no files exist", async () => {
      const repoPath = tempDir;

      const status = await getCodexHooksStatus(repoPath);

      expect(status.installed).toBe(false);
      expect(status.hasPromptHook).toBe(false);
      expect(status.hasStopHook).toBe(false);
      expect(status.hasHooksJsonEntry).toBe(false);
      expect(status.hasFeatureFlag).toBe(false);
    });

    it("should detect installed status correctly", async () => {
      const repoPath = tempDir;
      await setupCodexHooks(repoPath, "task-1", "http://localhost:3000");

      const status = await getCodexHooksStatus(repoPath);

      expect(status).toEqual({
        installed: true,
        hasPromptHook: true,
        hasStopHook: true,
        hasHooksJsonEntry: true,
        hasFeatureFlag: true,
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
