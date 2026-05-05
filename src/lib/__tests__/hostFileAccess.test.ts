import { execFile } from "child_process";
import { mkdtemp, readFile, rm, writeFile } from "fs/promises";
import { tmpdir } from "os";
import path from "path";
import { promisify } from "util";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const execFileAsync = promisify(execFile);

const mocks = vi.hoisted(() => ({
  execGit: vi.fn(),
}));

vi.mock("@/lib/gitOperations", () => ({
  execGit: mocks.execGit,
}));

function quoteForPosixShell(value: string): string {
  return `'${value.replace(/'/g, `'"'"'`)}'`;
}

function wrapLikeRemoteExecGit(command: string): string {
  const marker = "__KANVIBE_TEST_REMOTE_COMMAND_EXIT__";
  const wrappedCommand = [
    `sh -lc ${quoteForPosixShell(command)}`,
    "__kanvibe_status=$?",
    `printf '\\n${marker}:%s\\n' "$__kanvibe_status"`,
    'exit "$__kanvibe_status"',
  ].join("; ");

  return `sh -lc ${quoteForPosixShell(wrappedCommand)}`;
}

describe("hostFileAccess.readTextFiles", () => {
  let tempDir: string;

  beforeEach(async () => {
    tempDir = await mkdtemp(path.join(tmpdir(), "kanvibe-host-files-"));
    mocks.execGit.mockReset();
  });

  afterEach(async () => {
    await rm(tempDir, { recursive: true, force: true });
  });

  it("원격 파일 묶음 읽기 명령은 POSIX sh에서 실행 가능해야 한다", async () => {
    // Given
    const filePath = path.join(tempDir, "hook file's config.txt");
    const missingPath = path.join(tempDir, "missing.txt");
    const content = "line 1\nline 2\n";
    await writeFile(filePath, content, "utf-8");
    mocks.execGit.mockImplementation(async (command: string) => {
      const { stdout } = await execFileAsync("sh", ["-lc", command]);
      return stdout;
    });
    const { readTextFiles } = await import("@/lib/hostFileAccess");

    // When
    const files = await readTextFiles([filePath, missingPath], "remote-host");

    // Then
    expect(files.get(filePath)).toEqual({ exists: true, content });
    expect(files.get(missingPath)).toEqual({ exists: false, content: "" });
    expect(mocks.execGit).toHaveBeenCalledWith(expect.any(String), "remote-host");
    await expect(readFile(filePath, "utf-8")).resolves.toBe(content);
  });

  it("원격 파일 묶음 읽기 명령은 실제 SSH 이중 shell wrapper에서도 실행 가능해야 한다", async () => {
    // Given
    const filePath = path.join(tempDir, "hook file's config.txt");
    const missingPath = path.join(tempDir, "missing file.txt");
    const content = "line 1\nline 2\n";
    await writeFile(filePath, content, "utf-8");
    mocks.execGit.mockImplementation(async (command: string) => {
      const { stdout } = await execFileAsync("sh", ["-lc", wrapLikeRemoteExecGit(command)]);
      return stdout;
    });
    const { readTextFiles } = await import("@/lib/hostFileAccess");

    // When
    const files = await readTextFiles([filePath, missingPath], "remote-host");

    // Then
    expect(files.get(filePath)).toEqual({ exists: true, content });
    expect(files.get(missingPath)).toEqual({ exists: false, content: "" });
  });
});
