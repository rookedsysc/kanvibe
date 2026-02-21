import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { mkdtemp, readFile, rm } from "fs/promises";
import { join } from "path";
import { tmpdir } from "os";
import { setupOpenCodeHooks, getOpenCodeHooksStatus } from "../openCodeHooksSetup";

describe("openCodeHooksSetup", () => {
  let tempDir: string;

  beforeEach(async () => {
    tempDir = await mkdtemp(join(tmpdir(), "opencode-test-"));
  });

  afterEach(async () => {
    await rm(tempDir, { recursive: true, force: true });
  });

  describe("setupOpenCodeHooks - file operations", () => {
    it("should create plugin file at .opencode/plugins/kanvibe-plugin.ts", async () => {
      // Given
      const repoPath = tempDir;

      // When
      await setupOpenCodeHooks(repoPath, "test-project", "http://localhost:3000");

      // Then
      const status = await getOpenCodeHooksStatus(repoPath);
      expect(status.hasPlugin).toBe(true);
    });

    it("should generate plugin file containing KanvibePlugin and /api/hooks/status", async () => {
      // Given
      const repoPath = tempDir;

      // When
      await setupOpenCodeHooks(repoPath, "test-project", "http://localhost:3000");

      // Then
      const pluginPath = join(repoPath, ".opencode", "plugins", "kanvibe-plugin.ts");
      const pluginContent = await readFile(pluginPath, "utf-8");

      expect(pluginContent).toContain("KanvibePlugin");
      expect(pluginContent).toContain("/api/hooks/status");
    });

    it("should not fail when called twice (overwrites existing plugin)", async () => {
      // Given
      const repoPath = tempDir;
      await setupOpenCodeHooks(repoPath, "test-project", "http://localhost:3000");

      // When - setup again
      await setupOpenCodeHooks(repoPath, "test-project", "http://localhost:3000");

      // Then - should still be installed correctly
      const status = await getOpenCodeHooksStatus(repoPath);
      expect(status.installed).toBe(true);
    });
  });

  describe("getOpenCodeHooksStatus", () => {
    it("should return installed: true after setup", async () => {
      // Given
      const repoPath = tempDir;
      await setupOpenCodeHooks(repoPath, "test-project", "http://localhost:3000");

      // When
      const status = await getOpenCodeHooksStatus(repoPath);

      // Then
      expect(status.installed).toBe(true);
    });

    it("should return installed: false when no plugin exists", async () => {
      // Given
      const repoPath = tempDir;

      // When
      const status = await getOpenCodeHooksStatus(repoPath);

      // Then
      expect(status.installed).toBe(false);
      expect(status.hasPlugin).toBe(false);
    });
  });
});
