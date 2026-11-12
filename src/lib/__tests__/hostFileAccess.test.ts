import { execFile, spawn } from "child_process";
import { chmod, mkdir, mkdtemp, readFile, rm, stat, symlink, writeFile } from "fs/promises";
import { tmpdir } from "os";
import path from "path";
import { promisify } from "util";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const execFileAsync = promisify(execFile);

const mocks = vi.hoisted(() => ({
  execGit: vi.fn(),
  skipsChmod: false,
  readError: undefined as NodeJS.ErrnoException | undefined,
  stdinReplies: [] as string[],
}));

/** 기록 뒤 권한 조정을 건너뛰게 해, 파일이 처음 만들어질 때의 권한을 볼 수 있게 한다 */
vi.mock("fs/promises", async () => {
  /** 이 환경에서는 importActual("fs/promises")가 빈 모듈을 돌려주므로, 가로채지 않는 node: 경로로 실제 모듈을 읽는다 */
  const actual = await import("node:fs/promises");
  const chmod = async (...args: Parameters<typeof actual.chmod>) => (mocks.skipsChmod ? undefined : actual.chmod(...args));
  const readFile = async (...args: Parameters<typeof actual.readFile>) => {
    if (mocks.readError) throw mocks.readError;
    return actual.readFile(...args);
  };
  return { ...actual, chmod, readFile, default: { ...actual, chmod, readFile } };
});

vi.mock("@/lib/gitOperations", () => ({
  execGit: mocks.execGit,
  resolvePathForShell: (targetPath: string) => quoteForPosixShell(targetPath),
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

describe("hostFileAccess.isPathMissing", () => {
  let directory: string;

  beforeEach(async () => {
    directory = await mkdtemp(path.join(tmpdir(), "kanvibe-presence-"));
    mocks.execGit.mockReset();
    mocks.execGit.mockImplementation(async (command: string) => {
      const { stdout } = await execFileAsync("sh", ["-lc", command]);
      return stdout.trim();
    });
  });

  afterEach(async () => {
    await chmod(path.join(directory, "locked"), 0o700).catch(() => undefined);
    await rm(directory, { recursive: true, force: true });
  });

  it.each([null, "remote-host"])("없는 중첩 경로만 삭제된 것으로 판별한다: %s", async (host) => {
    const { isPathMissing } = await import("@/lib/hostFileAccess");
    const missing = path.join(directory, "repo's folder", "worktree");
    await expect(isPathMissing(directory, host)).resolves.toBe(false);
    await expect(isPathMissing(missing, host)).resolves.toBe(true);
    await expect(stat(missing)).rejects.toMatchObject({ code: "ENOENT" });
  });

  it.each([null, "remote-host"])("끊어진 심볼릭 링크는 삭제로 취급하지 않는다: %s", async (host) => {
    const { isPathMissing } = await import("@/lib/hostFileAccess");
    const link = path.join(directory, "repo-link");
    await symlink(path.join(directory, "removed-target"), link);
    await expect(isPathMissing(link, host)).resolves.toBe(false);
  });

  it.each([null, "remote-host"])("상위 디렉토리 접근이 거절되면 삭제로 취급하지 않는다: %s", async (host) => {
    const { isPathMissing } = await import("@/lib/hostFileAccess");
    const locked = path.join(directory, "locked");
    await mkdir(locked);
    await chmod(locked, 0o000);
    await expect(isPathMissing(path.join(locked, "repo"), host)).rejects.toThrow();
  });

  it("SSH 접속 실패를 삭제로 취급하지 않는다", async () => {
    const { isPathMissing } = await import("@/lib/hostFileAccess");
    mocks.execGit.mockRejectedValue(new Error("SSH connection refused"));
    await expect(isPathMissing("/workspace/repo", "offline-host")).rejects.toThrow("SSH connection refused");
  });
});

describe("hostFileAccess.readTextFiles", () => {
  let tempDir: string;

  beforeEach(async () => {
    tempDir = await mkdtemp(path.join(tmpdir(), "kanvibe-host-files-"));
    mocks.execGit.mockReset();
    mocks.readError = undefined;
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
      return stdout.trim();
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
      return stdout.slice(0, stdout.lastIndexOf("\n__KANVIBE_TEST_REMOTE_COMMAND_EXIT__:")).trim();
    });
    const { readTextFiles } = await import("@/lib/hostFileAccess");

    // When
    const files = await readTextFiles([filePath, missingPath], "remote-host");

    // Then
    expect(files.get(filePath)).toEqual({ exists: true, content });
    expect(files.get(missingPath)).toEqual({ exists: false, content: "" });
  });

  it.each([false, true])("SSH 출력이 trim되어도 단일 파일의 빈 내용을 보존한다: exists=%s", async (exists) => {
    const filePath = path.join(tempDir, "empty.txt");
    if (exists) await writeFile(filePath, "", "utf-8");
    mocks.execGit.mockImplementation(async (command: string) => {
      const { stdout } = await execFileAsync("sh", ["-lc", command]);
      return stdout.trim();
    });
    const { readTextFiles } = await import("@/lib/hostFileAccess");

    await expect(readTextFiles([filePath], "remote-host")).resolves.toEqual(
      new Map([[filePath, { exists, content: "" }]]),
    );
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

  it("없는 로컬 파일만 missing으로 반환하고 권한 오류는 전파한다", async () => {
    const { readTextFiles } = await import("@/lib/hostFileAccess");
    const missingPath = path.join(tempDir, "missing.txt");
    await expect(readTextFiles([missingPath])).resolves.toEqual(new Map([[missingPath, { exists: false, content: "" }]]));
    mocks.readError = Object.assign(new Error("permission denied"), { code: "EACCES" });
    await expect(readTextFiles([missingPath])).rejects.toThrow("permission denied");
    mocks.readError = undefined;
  });

  it("로컬 디렉터리 읽기 실패를 파일 부재로 숨기지 않는다", async () => {
    const { readTextFiles } = await import("@/lib/hostFileAccess");
    await expect(readTextFiles([tempDir])).rejects.toThrow();
  });

  it.each([
    "", "__KANVIBE_FILE_RECORD__\t/remote/a\t1\t@@@\t__KANVIBE_FILE_RECORD_END__\n",
    "__KANVIBE_FILE_RECORD__\t/remote/a\t1\taGk=\t__KANVIBE_FILE_RECORD_END__\n",
    "__KANVIBE_FILE_RECORD__\t/remote/a\t2\t\t__KANVIBE_FILE_RECORD_END__\n__KANVIBE_FILE_RECORD__\t/remote/b\t0\t\t__KANVIBE_FILE_RECORD_END__\n",
    "__KANVIBE_FILE_RECORD__\t/remote/a\t0\t\tbroken\n__KANVIBE_FILE_RECORD__\t/remote/b\t0\t\t__KANVIBE_FILE_RECORD_END__\n",
  ])("누락되거나 손상된 원격 레코드를 설치 부재로 판정하지 않는다: %s", async (output) => {
    mocks.execGit.mockResolvedValue(output);
    const { readTextFiles } = await import("@/lib/hostFileAccess");
    await expect(readTextFiles(["/remote/a", "/remote/b"], "remote-host")).rejects.toThrow();
  });

  it("원격 부모 디렉터리를 탐색할 수 없으면 missing으로 숨기지 않는다", async () => {
    const blockedDirectory = path.join(tempDir, "blocked");
    await mkdir(blockedDirectory);
    const filePath = path.join(blockedDirectory, "config.txt");
    await writeFile(filePath, "current", "utf-8");
    await chmod(blockedDirectory, 0o000);
    mocks.execGit.mockImplementation(async (command: string) => {
      const { stdout } = await execFileAsync("sh", ["-c", command]);
      return stdout;
    });
    try {
      const { readTextFiles } = await import("@/lib/hostFileAccess");
      await expect(readTextFiles([filePath], "remote-host")).rejects.toThrow();
    } finally {
      await chmod(blockedDirectory, 0o700);
    }
  });

  it("실제 원격 shell의 base64 읽기 실패가 성공으로 숨겨지지 않는다", async () => {
    const filePath = path.join(tempDir, "config.txt");
    await writeFile(filePath, "current", "utf-8");
    mocks.execGit.mockImplementation(async (command: string) => {
      const failFileEncoding = 'base64() { case "$*" in "-w 0 "*) return 42;; *) if [ "$#" -eq 0 ]; then return 42; fi; command base64 "$@";; esac; }; ';
      const { stdout } = await execFileAsync("sh", ["-c", failFileEncoding + command]);
      return stdout;
    });
    const { readTextFiles } = await import("@/lib/hostFileAccess");
    await expect(readTextFiles([filePath], "remote-host")).rejects.toThrow();
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
    mocks.stdinReplies = [];
  });

  afterEach(async () => {
    await rm(tempDir, { recursive: true, force: true });
  });

  async function readFileMode(targetPath: string): Promise<number> {
    return (await stat(targetPath)).mode & 0o777;
  }

  function runRemoteCommandsWithoutChmod(): void {
    mocks.execGit.mockImplementation(async (command: string, _sshHost: string, options?: { maxBufferBytes?: number; remoteStdin?: string; remoteStdinResponder?: { marker: string; respond: (payload: string) => Promise<string> } }) => {
      const commandWithoutChmod = `umask 022; chmod() { :; }; ${command}`;
      const child = spawn("sh", ["-c", commandWithoutChmod], {
        stdio: [options?.remoteStdin === undefined && !options?.remoteStdinResponder ? "ignore" : "pipe", "pipe", "pipe"],
      });
      let callbackError: unknown;
      child.stdin?.on("error", () => undefined);
      const completion = new Promise<string>((resolve, reject) => {
        let stdout = "";
        let replied = false;
        child.stdout?.on("data", (chunk: string) => {
          stdout += chunk;
          if (Buffer.byteLength(stdout) > (options?.maxBufferBytes ?? 10 * 1024 * 1024)) {
            callbackError = new Error("SSH command output exceeded maxBuffer");
            child.stdin?.end();
          }
          if (callbackError) return;
          const responder = options?.remoteStdinResponder;
          const line = stdout.split("\n").find((value) => value.startsWith(`${responder?.marker}\t`));
          if (!replied && responder && line !== undefined && stdout.includes(`${line}\n`)) {
            replied = true;
            void responder.respond(line.slice(responder.marker.length + 1)).then((reply) => {
              mocks.stdinReplies.push(reply);
              child.stdin?.end(reply);
            }, (error) => { callbackError = error; child.stdin?.end(); });
          }
        });
        child.on("close", (code) => callbackError ? reject(callbackError) : code === 0 ? resolve(stdout.trim()) : reject(new Error(`exit ${code}: ${command}`)));
      });
      if (!options?.remoteStdinResponder) child.stdin?.end(options?.remoteStdin);
      return completion;
    });
  }

  it("비동기 갱신이 끝날 때까지 잠금을 유지하고 동시 변경을 모두 보존한다", async () => {
    const targetPath = path.join(tempDir, "async-map.json");
    const { updateTextFile } = await import("@/lib/hostFileAccess");
    await Promise.all(["a", "b"].map((letter) => updateTextFile(targetPath, async (content) => {
      await Promise.resolve();
      return content + letter;
    }, null, 0o600)));
    expect((await readFile(targetPath, "utf-8")).split("").sort()).toEqual(["a", "b"]);
  });

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
    expect(mocks.stdinReplies.some((reply) => reply.includes(encodedSecret))).toBe(true);
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
    await expect(stat(`${targetPath}.lock`)).rejects.toMatchObject({ code: "ENOENT" });
  });

  it("원격의 현재 내용은 공백과 마지막 줄바꿈을 보존하고 한 SSH transaction으로 갱신한다", async () => {
    const targetPath = path.join(tempDir, "exact.txt");
    await writeFile(targetPath, "  saved\n\n");
    runRemoteCommandsWithoutChmod();
    const { updateTextFile } = await import("@/lib/hostFileAccess");
    const update = vi.fn(async (content: string) => `${content}next\n`);
    await updateTextFile(targetPath, update, "remote-host", 0o600);
    expect(update).toHaveBeenCalledExactlyOnceWith("  saved\n\n");
    expect(mocks.execGit).toHaveBeenCalledTimes(1);
    await expect(readFile(targetPath, "utf-8")).resolves.toBe("  saved\n\nnext\n");
  });

  it("8MiB 현재 파일의 base64 요청도 callback에 전달하고 더 작은 결과로 교체한다", async () => {
    const targetPath = path.join(tempDir, "large-current.txt");
    const current = "x".repeat(8 * 1024 * 1024);
    await writeFile(targetPath, current);
    runRemoteCommandsWithoutChmod();
    const { updateTextFile } = await import("@/lib/hostFileAccess");
    const update = vi.fn(async (content: string) => {
      expect(content).toBe(current);
      return "small result";
    });
    await updateTextFile(targetPath, update, "remote-host", 0o600);
    expect(update).toHaveBeenCalledTimes(1);
    await expect(readFile(targetPath, "utf-8")).resolves.toBe("small result");
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


describe("hostFileAccess strict SSH boundaries", () => {
  beforeEach(() => { mocks.execGit.mockReset(); mocks.readError = undefined; });

  it("hook 파일 읽기와 쓰기는 명시한 strict 옵션을 SSH 경계까지 전달한다", async () => {
    const files = await import("@/lib/hostFileAccess");
    mocks.execGit.mockResolvedValue("");
    const options = { strictHostKeyChecking: true };
    await files.readTextFile("/remote/config", "remote-host", options);
    expect(mocks.execGit).toHaveBeenLastCalledWith(expect.any(String), "remote-host", { ...options, maxBufferBytes: 64 * 1024 * 1024 });
    mocks.execGit.mockResolvedValueOnce("__KANVIBE_FILE_RECORD__\t/remote/config\t0\t\t__KANVIBE_FILE_RECORD_END__\n");
    await files.readTextFiles(["/remote/config"], "remote-host", options);
    await files.findSymbolicLinkPath(["/remote/config"], "remote-host", options);
    await files.writeTextFile("/remote/config", "content", "remote-host", options);
    mocks.execGit.mockImplementation(async (_command: string, _host: string, executionOptions?: { remoteStdinResponder?: { respond: (payload: string) => Promise<string> } }) => {
      await executionOptions?.remoteStdinResponder?.respond("");
      return "__KANVIBE_FILE_UPDATED__";
    });
    await files.updateTextFile("/remote/config", () => "content", "remote-host", 0o600, options);
    expect(mocks.execGit.mock.calls.length).toBe(5);
    for (const call of mocks.execGit.mock.calls) {
      expect(call[2]).toMatchObject(options);
    }
    expect(mocks.execGit.mock.calls.some((call) => call[2]?.remoteStdinResponder)).toBe(true);
  });
});
