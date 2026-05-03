import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  homedir: vi.fn(() => "/home/local-user"),
  exec: vi.fn(),
  execFile: vi.fn(),
  mkdir: vi.fn(async () => undefined),
}));

vi.mock("child_process", () => ({
  default: {
    exec: mocks.exec,
    execFile: mocks.execFile,
  },
  exec: mocks.exec,
  execFile: mocks.execFile,
}));

vi.mock("os", () => ({
  default: {
    homedir: mocks.homedir,
  },
  homedir: mocks.homedir,
}));

vi.mock("fs/promises", () => ({
  default: {
    mkdir: mocks.mkdir,
    readFile: vi.fn(),
  },
  mkdir: mocks.mkdir,
  readFile: vi.fn(),
}));

describe("gitOperations.resolvePathForShell", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
  });

  it("로컬 틸드 경로는 로컬 홈 디렉토리로 확장한다", async () => {
    // Given
    const { resolvePathForShell } = await import("@/lib/gitOperations");

    // When
    const result = resolvePathForShell("~/work");

    // Then
    expect(result).toBe('"/home/local-user/work"');
  });

  it("원격 틸드 경로는 원격 HOME 기준으로 검색한다", async () => {
    // Given
    const { resolvePathForShell } = await import("@/lib/gitOperations");

    // When
    const result = resolvePathForShell("~/work", "remote-host");

    // Then
    expect(result).toBe('"$HOME/work"');
  });

  it("로컬 명령은 macOS 앱 실행 환경에서 누락되기 쉬운 CLI 경로를 포함한다", async () => {
    // Given
    mocks.exec.mockImplementation((_command: string, _options: unknown, callback: (error: null, result: { stdout: string; stderr: string }) => void) => {
      callback(null, { stdout: "ok\n", stderr: "" });
    });
    const { execGit } = await import("@/lib/gitOperations");

    // When
    const result = await execGit("command -v tmux", null);

    // Then
    expect(result).toBe("ok");
    expect(mocks.exec).toHaveBeenCalledWith(
      "command -v tmux",
      expect.objectContaining({
        env: expect.objectContaining({
          PATH: process.platform === "darwin"
            ? expect.stringContaining("/opt/homebrew/bin")
            : expect.any(String),
        }),
      }),
      expect.any(Function),
    );
  });

  it("원격 명령 실행은 ssh 바이너리와 옵션을 사용한다", async () => {
    // Given
    mocks.execFile.mockImplementation((_file: string, _args: string[], _options: unknown, callback: (error: null, result: { stdout: string }) => void) => {
      callback(null, { stdout: "ok\n" });
    });

    vi.doMock("@/lib/sshConfig", async (importOriginal) => {
      const actual = await importOriginal<typeof import("@/lib/sshConfig")>();
      return {
        ...actual,
        parseSSHConfig: vi.fn(async () => [{
          host: "remote-host",
          hostname: "example.com",
          port: 2202,
          username: "tester",
          privateKeyPath: "/tmp/test-key",
        }]),
      };
    });

    const { execGit } = await import("@/lib/gitOperations");

    // When
    const result = await execGit("pwd", "remote-host");

    // Then
    const expectedArgs = [
      "-i",
      "/tmp/test-key",
      "-p",
      "2202",
      "-o",
      "BatchMode=yes",
      "-o",
      "IdentitiesOnly=yes",
      "-T",
    ];

    if (process.platform !== "win32") {
      expectedArgs.push(
        "-o",
        "ControlMaster=auto",
        "-o",
        "ControlPersist=10m",
        "-o",
        "ControlPath=/home/local-user/.kanvibe/ssh-%C",
      );
    }

    expectedArgs.push(
      "remote-host",
      "sh -lc 'pwd'",
    );

    expect(result).toBe("ok");
    expect(expectedArgs).not.toContain("-Y");
    expect(mocks.execFile).toHaveBeenCalledWith(
      "ssh",
      expectedArgs,
      expect.objectContaining({ maxBuffer: 10 * 1024 * 1024 }),
      expect.any(Function),
    );
  });

  it("SSH transport 실패 직후 같은 host의 후속 명령은 새 ssh 프로세스를 만들지 않는다", async () => {
    // Given
    const consoleErrorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    mocks.execFile.mockImplementation((_file: string, _args: string[], _options: unknown, callback: (error: Error & { stderr?: string }) => void) => {
      const error = new Error("Command failed") as Error & { stderr?: string };
      error.stderr = [
        "mux_client_request_session: session request failed: Session open refused by peer",
        "Connection closed by 100.73.171.123 port 22",
      ].join("\n");
      callback(error);
    });

    vi.doMock("@/lib/sshConfig", async (importOriginal) => {
      const actual = await importOriginal<typeof import("@/lib/sshConfig")>();
      return {
        ...actual,
        parseSSHConfig: vi.fn(async () => [{
          host: "remote-host",
          hostname: "example.com",
          port: 2202,
          username: "tester",
          privateKeyPath: "/tmp/test-key",
        }]),
      };
    });

    const { execGit } = await import("@/lib/gitOperations");

    try {
      // When
      await expect(execGit("git -C /repo worktree list --porcelain", "remote-host")).rejects.toThrow();
      await expect(execGit("git -C /repo status --short", "remote-host")).rejects.toThrow();

      // Then
      expect(mocks.execFile).toHaveBeenCalledTimes(1);
    } finally {
      consoleErrorSpy.mockRestore();
    }
  });

  it("같은 SSH host의 원격 명령은 한 번에 하나씩 실행한다", async () => {
    // Given
    const startedCommands: string[] = [];
    const pendingCallbacks: Array<(error: null, result: { stdout: string }) => void> = [];
    mocks.execFile.mockImplementation((_file: string, args: string[], _options: unknown, callback: (error: null, result: { stdout: string }) => void) => {
      startedCommands.push(args.at(-1) ?? "");
      pendingCallbacks.push(callback);
    });

    vi.doMock("@/lib/sshConfig", async (importOriginal) => {
      const actual = await importOriginal<typeof import("@/lib/sshConfig")>();
      return {
        ...actual,
        parseSSHConfig: vi.fn(async () => [{
          host: "remote-host",
          hostname: "example.com",
          port: 2202,
          username: "tester",
          privateKeyPath: "/tmp/test-key",
        }]),
      };
    });

    const { execGit } = await import("@/lib/gitOperations");

    // When
    const first = execGit("first", "remote-host");
    const second = execGit("second", "remote-host");
    await new Promise((resolve) => setTimeout(resolve, 0));

    // Then
    expect(startedCommands).toEqual(["sh -lc 'first'"]);

    pendingCallbacks.shift()?.(null, { stdout: "one\n" });
    await expect(first).resolves.toBe("one");
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(startedCommands).toEqual(["sh -lc 'first'", "sh -lc 'second'"]);

    pendingCallbacks.shift()?.(null, { stdout: "two\n" });
    await expect(second).resolves.toBe("two");
  });

  it("조용한 probe 명령 실패는 콘솔 에러를 남기지 않는다", async () => {
    // Given
    const consoleErrorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    mocks.execFile.mockImplementation((_file: string, _args: string[], _options: unknown, callback: (error: Error & { stderr?: string }) => void) => {
      const error = new Error("Command failed") as Error & { stderr?: string };
      error.stderr = "";
      callback(error);
    });

    vi.doMock("@/lib/sshConfig", async (importOriginal) => {
      const actual = await importOriginal<typeof import("@/lib/sshConfig")>();
      return {
        ...actual,
        parseSSHConfig: vi.fn(async () => [{
          host: "remote-host",
          hostname: "example.com",
          port: 2202,
          username: "tester",
          privateKeyPath: "/tmp/test-key",
        }]),
      };
    });

    const { execGit } = await import("@/lib/gitOperations");

    // When
    await expect(execGit('tmux has-session -t "remote-session" 2>/dev/null', "remote-host")).rejects.toThrow();

    // Then
    expect(consoleErrorSpy).not.toHaveBeenCalled();
  });

  it("파일 probe 명령 실패도 콘솔 에러를 남기지 않는다", async () => {
    // Given
    const consoleErrorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    mocks.execFile.mockImplementation((_file: string, _args: string[], _options: unknown, callback: (error: Error & { stderr?: string }) => void) => {
      const error = new Error("Command failed") as Error & { stderr?: string };
      error.stderr = "Connection reset by peer\n";
      callback(error);
    });

    vi.doMock("@/lib/sshConfig", async (importOriginal) => {
      const actual = await importOriginal<typeof import("@/lib/sshConfig")>();
      return {
        ...actual,
        parseSSHConfig: vi.fn(async () => [{
          host: "remote-host",
          hostname: "example.com",
          port: 2202,
          username: "tester",
          privateKeyPath: "/tmp/test-key",
        }]),
      };
    });

    const { execGit } = await import("@/lib/gitOperations");

    // When
    await expect(execGit("test -f '/repo/.kanvibe/task-id' && cat '/repo/.kanvibe/task-id' || true", "remote-host")).rejects.toThrow();

    // Then
    expect(consoleErrorSpy).not.toHaveBeenCalled();
  });

  it("SSH transport 오류를 원격 명령 내부 실패와 구분한다", async () => {
    // Given
    const { isSSHTransportError } = await import("@/lib/gitOperations");

    const connectionResetError = new Error("remote-host 원격 명령 실패: Connection reset by 100.73.171.123 port 22");
    const keyExchangeError = {
      stderr: "kex_exchange_identification: read: Connection reset by peer\n",
    };
    const commandError = new Error("remote-host 원격 명령 실패: tmux 설치에 실패했습니다.");

    // Then
    expect(isSSHTransportError(connectionResetError)).toBe(true);
    expect(isSSHTransportError(keyExchangeError)).toBe(true);
    expect(isSSHTransportError(commandError)).toBe(false);
  });

  it("remoteBranchExists는 origin branch가 있으면 true를 반환한다", async () => {
    // Given
    mocks.exec.mockImplementation((
      command: string,
      _options: unknown,
      callback: (error: null, result: { stdout: string; stderr: string }) => void,
    ) => {
      expect(command).toContain("refs/heads/feature/exists");
      callback(null, { stdout: "exists", stderr: "" });
      return {} as never;
    });

    const { remoteBranchExists } = await import("@/lib/gitOperations");

    // When
    const result = await remoteBranchExists("/workspace/repo", "feature/exists");

    // Then
    expect(result).toBe(true);
  });

  it("remoteBranchExists는 origin branch가 없으면 false를 반환한다", async () => {
    // Given
    mocks.exec.mockImplementation((
      command: string,
      _options: unknown,
      callback: (error: null, result: { stdout: string; stderr: string }) => void,
    ) => {
      expect(command).toContain("refs/heads/feature/missing");
      callback(null, { stdout: "missing", stderr: "" });
      return {} as never;
    });

    const { remoteBranchExists } = await import("@/lib/gitOperations");

    // When
    const result = await remoteBranchExists("/workspace/repo", "feature/missing");

    // Then
    expect(result).toBe(false);
  });

  it("scanGitRepos는 일반 저장소와 worktree 저장소를 모두 찾는다", async () => {
    // Given
    mocks.exec.mockImplementation((
      _command: string,
      _options: unknown,
      callback: (error: null, result: { stdout: string; stderr: string }) => void,
    ) => {
      callback(
        null,
        {
          stdout: "/workspace/api/.git\n/workspace/feature-worktree/.git\n",
          stderr: "",
        },
      );
      return {} as never;
    });

    const { scanGitRepos } = await import("@/lib/gitOperations");

    // When
    const result = await scanGitRepos("/workspace");

    // Then
    expect(mocks.exec).toHaveBeenCalledWith(
      'find "/workspace" -maxdepth 4 -name ".git" \\( -type d -o -type f \\) 2>/dev/null',
      expect.objectContaining({ env: expect.any(Object) }),
      expect.any(Function),
    );
    expect(result).toEqual(["/workspace/api", "/workspace/feature-worktree"]);
  });
});
