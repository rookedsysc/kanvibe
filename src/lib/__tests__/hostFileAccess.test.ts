import { execFile } from "child_process";
import { chmod, mkdtemp, readFile, rm, stat, writeFile } from "fs/promises";
import { tmpdir } from "os";
import path from "path";
import { promisify } from "util";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const execFileAsync = promisify(execFile);

const mocks = vi.hoisted(() => ({
  execGit: vi.fn(),
  skipsChmod: false,
}));

/** 기록 뒤 권한 조정을 건너뛰게 해, 파일이 처음 만들어질 때의 권한을 볼 수 있게 한다 */
vi.mock("fs/promises", async () => {
  /** 이 환경에서는 importActual("fs/promises")가 빈 모듈을 돌려주므로, 가로채지 않는 node: 경로로 실제 모듈을 읽는다 */
  const actual = await import("node:fs/promises");
  const chmod = async (...args: Parameters<typeof actual.chmod>) => (mocks.skipsChmod ? undefined : actual.chmod(...args));
  return { ...actual, chmod, default: { ...actual, chmod } };
});

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

describe("hostFileAccess.readTextFile", () => {
  beforeEach(() => {
    mocks.execGit.mockReset();
  });

  it("원격 채팅 파일은 일반 명령보다 큰 출력 한도로 읽는다", async () => {
    mocks.execGit.mockResolvedValue("remote chat");
    const { readTextFile } = await import("@/lib/hostFileAccess");

    await expect(readTextFile("/remote/chat.jsonl", "remote-host")).resolves.toBe("remote chat");
    expect(mocks.execGit).toHaveBeenCalledWith(
      "test -f '/remote/chat.jsonl' && cat '/remote/chat.jsonl' || true",
      "remote-host",
      { maxBufferBytes: 64 * 1024 * 1024 },
    );
  });

  it("원격 채팅 파일 읽기 실패를 빈 파일로 숨기지 않는다", async () => {
    mocks.execGit.mockRejectedValue(new Error("SSH command output exceeded maxBuffer"));
    const { readTextFile } = await import("@/lib/hostFileAccess");

    await expect(readTextFile("/remote/chat.jsonl", "remote-host")).rejects.toThrow("exceeded maxBuffer");
  });
});

describe("hostFileAccess.updateTextFile 권한", () => {
  let tempDir: string;

  beforeEach(async () => {
    tempDir = await mkdtemp(path.join(tmpdir(), "kanvibe-update-mode-"));
    mocks.execGit.mockReset();
    mocks.skipsChmod = false;
  });

  afterEach(async () => {
    await rm(tempDir, { recursive: true, force: true });
  });

  async function readFileMode(targetPath: string): Promise<number> {
    return (await stat(targetPath)).mode & 0o777;
  }

  function runRemoteCommandsWithoutChmod(): void {
    mocks.execGit.mockImplementation(async (command: string, _sshHost: string, options?: { remoteStdin?: string }) => {
      const commandWithoutChmod = `umask 022; chmod() { :; }; ${command}`;
      const child = execFile("sh", ["-c", commandWithoutChmod]);
      const completion = new Promise<string>((resolve, reject) => {
        let stdout = "";
        child.stdout?.on("data", (chunk: string) => { stdout += chunk; });
        child.on("close", (code) => code === 0 ? resolve(stdout) : reject(new Error(`exit ${code}: ${command}`)));
      });
      child.stdin?.end(options?.remoteStdin ?? "");
      return completion;
    });
  }

  it("임시 파일로 교체해도 자격 증명 파일은 처음부터 지정한 권한으로 공개된다", async () => {
    // Given
    mocks.skipsChmod = true;
    const targetPath = path.join(tempDir, "targets.json");
    const { updateTextFile } = await import("@/lib/hostFileAccess");

    // When
    await updateTextFile(targetPath, () => "{}", null, 0o600);

    // Then
    expect(await readFileMode(targetPath)).toBe(0o600);
    await expect(readFile(targetPath, "utf-8")).resolves.toBe("{}");
  });

  it("이미 넓은 권한으로 있던 파일을 갱신하면 지정한 권한으로 좁힌다", async () => {
    // Given
    const targetPath = path.join(tempDir, "targets.json");
    await writeFile(targetPath, "old", "utf-8");
    await chmod(targetPath, 0o644);
    const { updateTextFile } = await import("@/lib/hostFileAccess");

    // When
    await updateTextFile(targetPath, (content) => `${content}-next`, null, 0o600);

    // Then
    expect(await readFileMode(targetPath)).toBe(0o600);
    await expect(readFile(targetPath, "utf-8")).resolves.toBe("old-next");
  });

  it("원격 갱신도 chmod 전에 이미 지정한 권한으로 만들어진다", async () => {
    // Given
    const targetPath = path.join(tempDir, "targets.json");
    runRemoteCommandsWithoutChmod();
    const { updateTextFile } = await import("@/lib/hostFileAccess");

    // When
    await updateTextFile(targetPath, () => "{\"token\":\"secret\"}", "remote-host", 0o600);

    // Then
    expect(await readFileMode(targetPath)).toBe(0o600);
    await expect(readFile(targetPath, "utf-8")).resolves.toBe("{\"token\":\"secret\"}");
  });

  it("원격의 넓은 권한 파일을 큰 내용으로 갱신해도 내용과 권한이 지정한 대로 남는다", async () => {
    // Given
    const targetPath = path.join(tempDir, "targets.json");
    await writeFile(targetPath, "old", "utf-8");
    await chmod(targetPath, 0o644);
    const largeContent = "x".repeat(100 * 1024);
    runRemoteCommandsWithoutChmod();
    const { updateTextFile } = await import("@/lib/hostFileAccess");

    // When
    await updateTextFile(targetPath, () => largeContent, "remote-host", 0o600);

    // Then
    expect(await readFileMode(targetPath)).toBe(0o600);
    await expect(readFile(targetPath, "utf-8")).resolves.toBe(largeContent);
  });

  it("원격 갱신의 내용은 명령줄에 싣지 않고 표준 입력으로 보낸다", async () => {
    // Given
    const targetPath = path.join(tempDir, "targets.json");
    const secretContent = "{\"token\":\"secret-token\"}";
    const encodedSecret = Buffer.from(secretContent).toString("base64");
    runRemoteCommandsWithoutChmod();
    const { updateTextFile } = await import("@/lib/hostFileAccess");

    // When
    await updateTextFile(targetPath, () => secretContent, "remote-host", 0o600);

    // Then
    const remoteCommands = mocks.execGit.mock.calls.map(([command]) => command as string);
    expect(remoteCommands.some((command) => command.includes(encodedSecret) || command.includes("secret-token"))).toBe(false);
    expect(mocks.execGit.mock.calls.some(([, , options]) => options?.remoteStdin === encodedSecret)).toBe(true);
    await expect(readFile(targetPath, "utf-8")).resolves.toBe(secretContent);
  });

  it("원격이 표준 입력을 받지 못해 내용이 비면 기존 파일을 교체하지 않는다", async () => {
    // Given
    const targetPath = path.join(tempDir, "targets.json");
    await writeFile(targetPath, "existing", "utf-8");
    mocks.execGit.mockImplementation(async (command: string) => {
      if (command.includes("mv -f")) {
        await expect(execFileAsync("sh", ["-c", `{ ${command}; } < /dev/null`])).rejects.toThrow();
        throw new Error("publish failed");
      }
      return (await execFileAsync("sh", ["-c", command])).stdout;
    });
    const { updateTextFile } = await import("@/lib/hostFileAccess");

    // When
    const update = updateTextFile(targetPath, () => "next", "remote-host", 0o600);

    // Then
    await expect(update).rejects.toThrow("publish failed");
    await expect(readFile(targetPath, "utf-8")).resolves.toBe("existing");
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

  it("존재 검사 직후 다른 writer가 만든 원격 파일도 덮지 않는다", async () => {
    const targetPath = path.join(tempDir, "project's state.json");
    mocks.execGit.mockImplementation(async (command: string) => {
      const barrier = `function [() {
        builtin [ "$@"
        result=$?
        if builtin [ "$1" = "-e" ] && builtin [ "$result" -ne 0 ]; then
          printf 'authoritative\\n' > ${quoteForPosixShell(targetPath)}
        fi
        return "$result"
      }
`;
      return (await execFileAsync("bash", ["-c", barrier + command])).stdout;
    });
    const { writeTextFileIfAbsent } = await import("@/lib/hostFileAccess");

    await expect(writeTextFileIfAbsent(targetPath, "seed\n", "fixture-host")).resolves.toBeUndefined();

    expect(await readFile(targetPath, "utf8")).toBe("authoritative\n");
  });

});
