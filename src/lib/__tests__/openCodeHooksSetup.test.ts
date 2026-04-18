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
      await setupOpenCodeHooks(repoPath, "task-1", "http://localhost:3000");

      // Then
      const status = await getOpenCodeHooksStatus(repoPath);
      expect(status.hasPlugin).toBe(true);
    });

    it("should generate plugin file containing KanvibePlugin and /api/hooks/status", async () => {
      // Given
      const repoPath = tempDir;

      // When
      await setupOpenCodeHooks(repoPath, "task-1", "http://localhost:3000");

      // Then
      const pluginPath = join(repoPath, ".opencode", "plugins", "kanvibe-plugin.ts");
      const pluginContent = await readFile(pluginPath, "utf-8");

      expect(pluginContent).toContain("KanvibePlugin");
      expect(pluginContent).toContain("/api/hooks/status");
      expect(pluginContent).toContain('const DEFAULT_TASK_ID = "task-1";');
      expect(pluginContent).toContain('const TASK_ID_FILE = ".kanvibe/task-id";');
      expect(pluginContent).toContain("taskId: resolvedTaskId");
    });

    it("should generate plugin with all event handlers for status tracking", async () => {
      // Given
      const repoPath = tempDir;

      // When
      await setupOpenCodeHooks(repoPath, "task-1", "http://localhost:3000");

      // Then
      const pluginPath = join(repoPath, ".opencode", "plugins", "kanvibe-plugin.ts");
      const pluginContent = await readFile(pluginPath, "utf-8");

      expect(pluginContent).toContain('"message.updated"');
      expect(pluginContent).toContain('"question.asked"');
      expect(pluginContent).toContain('"question.replied"');
      expect(pluginContent).toContain('"session.idle"');
      expect(pluginContent).toContain('"session.deleted"');
    });

    it("should map event types to correct statuses", async () => {
      // Given
      const repoPath = tempDir;

      // When
      await setupOpenCodeHooks(repoPath, "task-1", "http://localhost:3000");

      // Then
      const pluginPath = join(repoPath, ".opencode", "plugins", "kanvibe-plugin.ts");
      const pluginContent = await readFile(pluginPath, "utf-8");

      expect(pluginContent).toMatch(/message\.updated[\s\S]*?role[\s\S]*?user[\s\S]*?progress/);
      expect(pluginContent).toMatch(/question\.asked[\s\S]*?pending/);
      expect(pluginContent).toMatch(/question\.replied[\s\S]*?progress/);
      expect(pluginContent).toMatch(/session\.idle[\s\S]*?review/);
      expect(pluginContent).toMatch(/session\.deleted[\s\S]*?done/);
    });

    it("should filter subagent sessions before updating statuses", async () => {
      // Given
      const repoPath = tempDir;

      // When
      await setupOpenCodeHooks(repoPath, "task-1", "http://localhost:3000");

      // Then
      const pluginPath = join(repoPath, ".opencode", "plugins", "kanvibe-plugin.ts");
      const pluginContent = await readFile(pluginPath, "utf-8");

      expect(pluginContent).toContain("const sessionCache = new Map<string, boolean>()");
      expect(pluginContent).toContain("function getSessionID(source: any): string | undefined");
      expect(pluginContent).toContain("function getParentSessionID(source: any): string | null | undefined");
      expect(pluginContent).toContain("client.session.get");
      expect(pluginContent).toContain("sessionCache.has(sessionID)");
      expect(pluginContent).toContain("sessionCache.set(sessionID, isMain)");
      expect(pluginContent).toContain("result.data?.parentID");
      expect(pluginContent).toContain("properties?.info ?? (event as any).properties?.message");
      expect(pluginContent).toContain("return sessionCache.get(sessionID) ?? false");
      expect(pluginContent).toContain("lastUserMessageBySession");
      expect(pluginContent).toContain("buildMessageSignature");
      expect(pluginContent).toContain("dedupeMessage: true");
      expect(pluginContent).toMatch(/message\.updated[\s\S]*?isMainSession\(message\)/);
      expect(pluginContent).toMatch(/question\.asked[\s\S]*?isMainSession\(event\.properties\)/);
      expect(pluginContent).toMatch(/question\.replied[\s\S]*?isMainSession\(event\.properties\)/);
      expect(pluginContent).toMatch(/session\.idle[\s\S]*?isMainSession\(event\.properties\)/);
      expect(pluginContent).toMatch(/session\.deleted[\s\S]*?isMainSession\(event\.properties\)/);
    });

    it("should not fail when called twice (overwrites existing plugin)", async () => {
      // Given
      const repoPath = tempDir;
      await setupOpenCodeHooks(repoPath, "task-1", "http://localhost:3000");

      // When - setup again
      await setupOpenCodeHooks(repoPath, "task-1", "http://localhost:3000");

      // Then - should still be installed correctly
      const status = await getOpenCodeHooksStatus(repoPath);
      expect(status.installed).toBe(true);
    });
  });

  describe("getOpenCodeHooksStatus", () => {
    it("should return installed: true after setup", async () => {
      // Given
      const repoPath = tempDir;
      await setupOpenCodeHooks(repoPath, "task-1", "http://localhost:3000");

      // When
      const status = await getOpenCodeHooksStatus(repoPath);

      // Then
      expect(status.installed).toBe(true);
      expect(status.hasDuplicateProgressGuard).toBe(true);
      expect(status.hasEventMappings).toBe(true);
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
