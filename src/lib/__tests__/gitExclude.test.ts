import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { mkdtemp, rm, readFile, writeFile, mkdir } from "fs/promises";
import { join } from "path";
import { tmpdir } from "os";
import { execSync } from "child_process";
import { addAiToolPatternsToGitExclude } from "../gitExclude";

describe("gitExclude", () => {
  let tempDir: string;

  beforeEach(async () => {
    tempDir = await mkdtemp(join(tmpdir(), "git-exclude-test-"));
    execSync("git init", { cwd: tempDir, stdio: "ignore" });
  });

  afterEach(async () => {
    await rm(tempDir, { recursive: true, force: true });
  });

  describe("addAiToolPatternsToGitExclude", () => {
    it("should create exclude file with marker block when it does not exist", async () => {
      // Given
      const excludePath = join(tempDir, ".git", "info", "exclude");
      await rm(excludePath, { force: true });

      // When
      await addAiToolPatternsToGitExclude(tempDir);

      // Then
      const content = await readFile(excludePath, "utf-8");
      expect(content).toContain("# KanVibe AI hooks (auto-generated)");
      expect(content).toContain(".claude/hooks/");
      expect(content).toContain(".claude/settings.json");
      expect(content).toContain(".gemini/hooks/");
      expect(content).toContain(".gemini/settings.json");
      expect(content).toContain(".codex/hooks/");
      expect(content).toContain(".codex/config.toml");
      expect(content).toContain(".opencode/plugins/");
    });

    it("should not duplicate patterns when called multiple times", async () => {
      // Given
      await addAiToolPatternsToGitExclude(tempDir);

      // When
      await addAiToolPatternsToGitExclude(tempDir);

      // Then
      const content = await readFile(
        join(tempDir, ".git", "info", "exclude"),
        "utf-8"
      );
      const markerCount = content.split("# KanVibe AI hooks (auto-generated)").length - 1;
      expect(markerCount).toBe(1);
    });

    it("should preserve existing content in exclude file", async () => {
      // Given
      const excludePath = join(tempDir, ".git", "info", "exclude");
      const existingContent = "# existing patterns\n*.log\n.env\n";
      await writeFile(excludePath, existingContent, "utf-8");

      // When
      await addAiToolPatternsToGitExclude(tempDir);

      // Then
      const content = await readFile(excludePath, "utf-8");
      expect(content).toContain("*.log");
      expect(content).toContain(".env");
      expect(content).toContain("# KanVibe AI hooks (auto-generated)");
      expect(content).toContain(".claude/hooks/");
    });

    it("should throw when path is not a git repository", async () => {
      // Given
      const nonGitDir = await mkdtemp(join(tmpdir(), "non-git-"));

      // When & Then
      await expect(addAiToolPatternsToGitExclude(nonGitDir)).rejects.toThrow();

      await rm(nonGitDir, { recursive: true, force: true });
    });

    it("should create info directory when it does not exist", async () => {
      // Given
      const infoDir = join(tempDir, ".git", "info");
      await rm(infoDir, { recursive: true, force: true });

      // When
      await addAiToolPatternsToGitExclude(tempDir);

      // Then
      const content = await readFile(
        join(infoDir, "exclude"),
        "utf-8"
      );
      expect(content).toContain("# KanVibe AI hooks (auto-generated)");
    });
  });
});
