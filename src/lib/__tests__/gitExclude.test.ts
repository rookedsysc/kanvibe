import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { mkdtemp, rm, readFile, writeFile } from "fs/promises";
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
      expect(content).not.toContain(".kanvibe/task-id");
      expect(content).toContain(".claude/hooks/");
      expect(content).toContain(".claude/settings.json");
      expect(content).toContain(".gemini/hooks/");
      expect(content).toContain(".gemini/settings.json");
      expect(content).toContain(".codex/hooks/");
      expect(content).toContain(".codex/hooks.json");
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

    it("should restore missing patterns when only the marker block remains", async () => {
      // Given
      const excludePath = join(tempDir, ".git", "info", "exclude");
      await writeFile(excludePath, "# KanVibe AI hooks (auto-generated)\n", "utf-8");

      // When
      await addAiToolPatternsToGitExclude(tempDir);

      // Then
      const content = await readFile(excludePath, "utf-8");
      expect(content).toContain("# KanVibe AI hooks (auto-generated)");
      expect(content).toContain(".claude/hooks/");
      expect(content).toContain(".codex/config.toml");
      expect(content).toContain(".opencode/plugins/");
    });

    it("should update the shared common-dir exclude when called from a linked worktree", async () => {
      // Given
      execSync('git config user.name "KanVibe Test"', { cwd: tempDir, stdio: "ignore" });
      execSync('git config user.email "kanvibe@example.com"', { cwd: tempDir, stdio: "ignore" });
      await writeFile(join(tempDir, "README.md"), "# test\n", "utf-8");
      execSync("git add README.md", { cwd: tempDir, stdio: "ignore" });
      execSync('git commit -m "init"', { cwd: tempDir, stdio: "ignore" });

      const worktreeDir = await mkdtemp(join(tmpdir(), "git-exclude-worktree-"));
      execSync(`git worktree add -b feature/test "${worktreeDir}" HEAD`, { cwd: tempDir, stdio: "ignore" });

      const commonDir = execSync("git rev-parse --path-format=absolute --git-common-dir", {
        cwd: worktreeDir,
        encoding: "utf-8",
      }).trim();
      const commonExcludePath = join(commonDir, "info", "exclude");
      await writeFile(commonExcludePath, "# KanVibe AI hooks (auto-generated)\n", "utf-8");

      try {
        // When
        await addAiToolPatternsToGitExclude(worktreeDir);

        // Then
        const content = await readFile(commonExcludePath, "utf-8");
        expect(content).toContain(".claude/hooks/");
        expect(content).toContain(".gemini/settings.json");
        expect(content).toContain(".codex/hooks.json");
      } finally {
        await rm(worktreeDir, { recursive: true, force: true });
      }
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

    it("should update the common git exclude when called from a worktree", async () => {
      // Given
      execSync("git config user.name 'Kanvibe Test'", { cwd: tempDir, stdio: "ignore" });
      execSync("git config user.email 'kanvibe@example.com'", { cwd: tempDir, stdio: "ignore" });
      await writeFile(join(tempDir, "README.md"), "# test\n", "utf-8");
      execSync("git add README.md", { cwd: tempDir, stdio: "ignore" });
      execSync("git commit -m 'init'", { cwd: tempDir, stdio: "ignore" });

      const worktreePath = join(
        tmpdir(),
        `git-exclude-worktree-${Date.now()}-${Math.random().toString(36).slice(2)}`
      );
      execSync(`git worktree add ${JSON.stringify(worktreePath)} -b worktree-test`, { cwd: tempDir, stdio: "ignore" });

      // When
      await addAiToolPatternsToGitExclude(worktreePath);

      // Then
      const content = await readFile(join(tempDir, ".git", "info", "exclude"), "utf-8");
      expect(content).toContain("# KanVibe AI hooks (auto-generated)");
      expect(content).toContain(".claude/hooks/");

      await rm(worktreePath, { recursive: true, force: true });
    });
  });
});
