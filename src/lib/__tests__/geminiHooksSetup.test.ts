import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { setupGeminiHooks, getGeminiHooksStatus } from "@/lib/geminiHooksSetup";
import { mkdir, rm, readFile, writeFile } from "fs/promises";
import path from "path";
import os from "os";

describe("geminiHooksSetup", () => {
  let tmpDir: string;

  beforeEach(async () => {
    tmpDir = path.join(os.tmpdir(), `kanvibe-test-${Date.now()}`);
    await mkdir(tmpDir, { recursive: true });
  });

  afterEach(async () => {
    await rm(tmpDir, { recursive: true, force: true });
  });

  describe("setupGeminiHooks", () => {
    it("should create hook scripts and settings.json in .gemini directory", async () => {
      // Given
      const repoPath = tmpDir;
      const projectName = "test-project";
      const kanvibeUrl = "http://localhost:4885";

      // When
      await setupGeminiHooks(repoPath, projectName, kanvibeUrl);

      // Then
      const promptScript = await readFile(
        path.join(repoPath, ".gemini/hooks/kanvibe-prompt-hook.sh"),
        "utf-8"
      );
      const stopScript = await readFile(
        path.join(repoPath, ".gemini/hooks/kanvibe-stop-hook.sh"),
        "utf-8"
      );
      const settings = JSON.parse(
        await readFile(path.join(repoPath, ".gemini/settings.json"), "utf-8")
      );

      expect(promptScript).toContain("#!/bin/bash");
      expect(promptScript).toContain("BeforeAgent");
      expect(promptScript).toContain(kanvibeUrl);
      expect(promptScript).toContain(projectName);
      expect(promptScript).toContain("echo '{}'");

      expect(stopScript).toContain("#!/bin/bash");
      expect(stopScript).toContain("AfterAgent");
      expect(stopScript).toContain("review");
      expect(stopScript).toContain("echo '{}'");

      expect(settings.hooks.BeforeAgent).toHaveLength(1);
      expect(settings.hooks.AfterAgent).toHaveLength(1);
      expect(settings.hooks.BeforeAgent[0].hooks[0].command).toContain(
        "kanvibe-prompt-hook.sh"
      );
      expect(settings.hooks.AfterAgent[0].hooks[0].command).toContain(
        "kanvibe-stop-hook.sh"
      );
    });

    it("should preserve existing settings.json entries when adding hooks", async () => {
      // Given
      const geminiDir = path.join(tmpDir, ".gemini");
      await mkdir(geminiDir, { recursive: true });
      await writeFile(
        path.join(geminiDir, "settings.json"),
        JSON.stringify({
          hooks: {
            BeforeAgent: [
              {
                matcher: "*",
                hooks: [
                  {
                    type: "command",
                    command: "existing-hook.sh",
                    timeout: 3000,
                  },
                ],
              },
            ],
          },
          customSetting: true,
        }),
        "utf-8"
      );

      // When
      await setupGeminiHooks(tmpDir, "test-project", "http://localhost:4885");

      // Then
      const settings = JSON.parse(
        await readFile(path.join(geminiDir, "settings.json"), "utf-8")
      );

      expect(settings.customSetting).toBe(true);
      expect(settings.hooks.BeforeAgent).toHaveLength(2);
      expect(settings.hooks.BeforeAgent[0].hooks[0].command).toBe(
        "existing-hook.sh"
      );
      expect(settings.hooks.BeforeAgent[1].hooks[0].command).toContain(
        "kanvibe-prompt-hook.sh"
      );
    });

    it("should not duplicate hooks when called multiple times", async () => {
      // Given
      await setupGeminiHooks(tmpDir, "test-project", "http://localhost:4885");

      // When
      await setupGeminiHooks(tmpDir, "test-project", "http://localhost:4885");

      // Then
      const settings = JSON.parse(
        await readFile(
          path.join(tmpDir, ".gemini/settings.json"),
          "utf-8"
        )
      );

      expect(settings.hooks.BeforeAgent).toHaveLength(1);
      expect(settings.hooks.AfterAgent).toHaveLength(1);
    });
  });

  describe("getGeminiHooksStatus", () => {
    it("should return installed: true after setupGeminiHooks", async () => {
      // Given
      await setupGeminiHooks(tmpDir, "test-project", "http://localhost:4885");

      // When
      const status = await getGeminiHooksStatus(tmpDir);

      // Then
      expect(status.installed).toBe(true);
      expect(status.hasPromptHook).toBe(true);
      expect(status.hasStopHook).toBe(true);
      expect(status.hasSettingsEntry).toBe(true);
    });

    it("should return installed: false when no hooks exist", async () => {
      // Given — empty directory, no .gemini folder

      // When
      const status = await getGeminiHooksStatus(tmpDir);

      // Then
      expect(status.installed).toBe(false);
      expect(status.hasPromptHook).toBe(false);
      expect(status.hasStopHook).toBe(false);
      expect(status.hasSettingsEntry).toBe(false);
    });

    it("should return installed: false when scripts exist but settings entry is missing", async () => {
      // Given
      const hooksDir = path.join(tmpDir, ".gemini/hooks");
      await mkdir(hooksDir, { recursive: true });
      await writeFile(path.join(hooksDir, "kanvibe-prompt-hook.sh"), "#!/bin/bash", "utf-8");
      await writeFile(path.join(hooksDir, "kanvibe-stop-hook.sh"), "#!/bin/bash", "utf-8");

      // When
      const status = await getGeminiHooksStatus(tmpDir);

      // Then
      expect(status.hasPromptHook).toBe(true);
      expect(status.hasStopHook).toBe(true);
      expect(status.hasSettingsEntry).toBe(false);
      expect(status.installed).toBe(false);
    });
  });
});
