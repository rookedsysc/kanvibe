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

  it("원격 파일 묶음 읽기는 manifest 마지막 파일도 누락하지 않는다", async () => {
    // Given
    const missingPath = path.join(tempDir, "missing.txt");
    const finalFilePath = path.join(tempDir, "hooks.json");
    const finalContent = JSON.stringify({ hooks: { Stop: [{ command: "custom" }] } });
    await writeFile(finalFilePath, finalContent, "utf-8");
    mocks.execGit.mockImplementation(async (command: string) => {
      const { stdout } = await execFileAsync("sh", ["-lc", command]);
      return stdout;
    });
    const { readTextFiles } = await import("@/lib/hostFileAccess");

    // When
    const files = await readTextFiles([missingPath, finalFilePath], "remote-host");

    // Then
    expect(files.get(missingPath)).toEqual({ exists: false, content: "" });
    expect(files.get(finalFilePath)).toEqual({ exists: true, content: finalContent });
  });
});

describe("hostFileAccess.writeTextFileIfAbsent", () => {
  let tempDir: string;

  beforeEach(async () => {
    tempDir = await mkdtemp(path.join(tmpdir(), "kanvibe-write-if-absent-"));
    mocks.execGit.mockReset();
  });

  afterEach(async () => {
    await rm(tempDir, { recursive: true, force: true });
  });

  it("파일이 없으면 상위 디렉터리까지 만들어 기록한다", async () => {
    // Given
    const targetPath = path.join(tempDir, "nested", "project.json");
    const { writeTextFileIfAbsent } = await import("@/lib/hostFileAccess");

    // When
    await writeTextFileIfAbsent(targetPath, "seed\n");

    // Then
    await expect(readFile(targetPath, "utf-8")).resolves.toBe("seed\n");
  });

  /** 이미 만들어진 값은 다른 주체가 확정한 권위 값이므로 씨앗 기록이 덮으면 안 된다 */
  it("이미 있는 파일은 덮지 않는다", async () => {
    // Given
    const targetPath = path.join(tempDir, "project.json");
    await writeFile(targetPath, "authoritative\n", "utf-8");
    const { writeTextFileIfAbsent } = await import("@/lib/hostFileAccess");

    // When
    await writeTextFileIfAbsent(targetPath, "seed\n");

    // Then
    await expect(readFile(targetPath, "utf-8")).resolves.toBe("authoritative\n");
  });

  it("원격 기록 명령은 POSIX sh에서 실행 가능하고 기존 파일을 덮지 않는다", async () => {
    // Given
    const createdPath = path.join(tempDir, "created's file.json");
    const existingPath = path.join(tempDir, "existing.json");
    await writeFile(existingPath, "authoritative\n", "utf-8");
    mocks.execGit.mockImplementation(async (command: string) => {
      const { stdout } = await execFileAsync("sh", ["-lc", wrapLikeRemoteExecGit(command)]);
      return stdout;
    });
    const { writeTextFileIfAbsent } = await import("@/lib/hostFileAccess");

    // When
    await writeTextFileIfAbsent(createdPath, "seed\n", "remote-host");
    await writeTextFileIfAbsent(existingPath, "seed\n", "remote-host");

    // Then
    await expect(readFile(createdPath, "utf-8")).resolves.toBe("seed\n");
    await expect(readFile(existingPath, "utf-8")).resolves.toBe("authoritative\n");
  });
});
