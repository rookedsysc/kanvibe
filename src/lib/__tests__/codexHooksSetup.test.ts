import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { mkdtemp, rm } from "fs/promises";
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
      await setupCodexHooks(repoPath, "test-project", "http://localhost:3000");

      // Then
      const status = await getCodexHooksStatus(repoPath);
      expect(status.hasNotifyHook).toBe(true);
    });

    it("should create config.toml with notify entry", async () => {
      // Given
      const repoPath = tempDir;

      // When
      await setupCodexHooks(repoPath, "test-project", "http://localhost:3000");

      // Then
      const status = await getCodexHooksStatus(repoPath);
      expect(status.hasConfigEntry).toBe(true);
    });

    it("should mark as installed when both hook and config exist", async () => {
      // Given
      const repoPath = tempDir;

      // When
      await setupCodexHooks(repoPath, "test-project", "http://localhost:3000");

      // Then
      const status = await getCodexHooksStatus(repoPath);
      expect(status.installed).toBe(true);
    });

    it("should not add duplicate notify entry", async () => {
      // Given
      const repoPath = tempDir;
      await setupCodexHooks(repoPath, "test-project", "http://localhost:3000");

      // When - setup again
      await setupCodexHooks(repoPath, "test-project", "http://localhost:3000");

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
      await setupCodexHooks(repoPath, "test-project", "http://localhost:3000");

      // When
      const status = await getCodexHooksStatus(repoPath);

      // Then
      expect(status).toEqual({
        installed: true,
        hasNotifyHook: true,
        hasConfigEntry: true,
      });
    });
  });
});
