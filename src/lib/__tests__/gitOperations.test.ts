import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  homedir: vi.fn(() => "/home/local-user"),
  exec: vi.fn(),
  execFile: vi.fn(),
  spawn: vi.fn(),
  mkdir: vi.fn(async () => undefined),
}));

vi.mock("child_process", () => ({
  default: {
    exec: mocks.exec,
    execFile: mocks.execFile,
    spawn: mocks.spawn,
  },
  exec: mocks.exec,
  execFile: mocks.execFile,
  spawn: mocks.spawn,
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

const REMOTE_COMMAND_EXIT_MARKER = "__KANVIBE_REMOTE_COMMAND_EXIT_7b3f6e5d__";

type MockListener = (...args: unknown[]) => void;

class MockReadableStream {
  private listeners = new Map<string, MockListener[]>();

  setEncoding = vi.fn();

  on(event: string, listener: MockListener): this {
    this.listeners.set(event, [...(this.listeners.get(event) ?? []), listener]);
    return this;
  }

  emit(event: string, ...args: unknown[]): void {
    for (const listener of this.listeners.get(event) ?? []) {
      listener(...args);
    }
  }
}

class MockSSHProcess {
  stdout = new MockReadableStream();
  stderr = new MockReadableStream();
  exitCode: number | null = null;
  killed = false;
  kill = vi.fn(() => {
    this.killed = true;
    return true;
  });
  private listeners = new Map<string, MockListener[]>();

  on(event: string, listener: MockListener): this {
    this.listeners.set(event, [...(this.listeners.get(event) ?? []), listener]);
    return this;
  }

  emit(event: string, ...args: unknown[]): void {
    for (const listener of this.listeners.get(event) ?? []) {
      listener(...args);
    }
  }

  writeStdout(chunk: string): void {
    this.stdout.emit("data", chunk);
  }

  writeStderr(chunk: string): void {
    this.stderr.emit("data", chunk);
  }

  emitError(error: Error): void {
    this.emit("error", error);
  }

  close(code: number | null = 0, signal: NodeJS.Signals | null = null): void {
    this.exitCode = code;
    this.emit("close", code, signal);
  }
}

function createMockSSHProcess(): MockSSHProcess {
  return new MockSSHProcess();
}

function completeRemoteCommand(child: MockSSHProcess, stdout: string, exitCode = 0): void {
  child.writeStdout(`${stdout}\n${REMOTE_COMMAND_EXIT_MARKER}:${exitCode}\n`);
}

function getSpawnedSSHArgs(callIndex = 0): string[] {
  return mocks.spawn.mock.calls[callIndex]?.[1] as string[] ?? [];
}

function getSpawnedRemoteCommand(callIndex = 0): string {
  return getSpawnedSSHArgs(callIndex).at(-1) ?? "";
}

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
    const spawnedChildren: MockSSHProcess[] = [];
    mocks.spawn.mockImplementation(() => {
      const child = createMockSSHProcess();
      spawnedChildren.push(child);
      queueMicrotask(() => completeRemoteCommand(child, "ok"));
      return child;
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
    const expectedPrefix = [
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
      expectedPrefix.push(
        "-o",
        "ControlMaster=auto",
        "-o",
        "ControlPersist=10m",
        "-o",
        "ControlPath=/home/local-user/.kanvibe/ssh-%C",
      );
    }

    expectedPrefix.push(
      "-o",
      "ConnectTimeout=8",
      "-o",
      "ServerAliveInterval=5",
      "-o",
      "ServerAliveCountMax=2",
    );

    expectedPrefix.push(
      "remote-host",
    );

    expect(result).toBe("ok");
    expect(spawnedChildren[0]?.killed).toBe(true);
    expect(expectedPrefix).not.toContain("-Y");
    expect(mocks.spawn).toHaveBeenCalledWith(
      "ssh",
      expect.arrayContaining(expectedPrefix),
      expect.objectContaining({
        env: expect.any(Object),
        stdio: ["ignore", "pipe", "pipe"],
      }),
    );
    expect(getSpawnedRemoteCommand()).toContain("pwd");
    expect(getSpawnedRemoteCommand()).toContain(REMOTE_COMMAND_EXIT_MARKER);
  });

  it("SSH transport 실패 직후 같은 host의 후속 명령은 새 ssh 프로세스를 만들지 않는다", async () => {
    // Given
    const consoleErrorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    mocks.execFile.mockImplementation((_file: string, _args: string[], _options: unknown, callback: (error: null, result: { stdout: string }) => void) => {
      callback(null, { stdout: "" });
    });
    mocks.spawn.mockImplementation(() => {
      const child = createMockSSHProcess();
      queueMicrotask(() => {
        child.writeStderr([
          "mux_client_request_session: session request failed: Session open refused by peer",
          "Connection closed by 100.73.171.123 port 22",
        ].join("\n"));
        child.close(255);
      });
      return child;
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
      expect(mocks.spawn).toHaveBeenCalledTimes(3);
      expect(mocks.execFile).toHaveBeenCalledTimes(3);
      expect(mocks.execFile.mock.calls[0]?.[1]).toEqual(expect.arrayContaining([
        "-O",
        "exit",
        "-S",
        "/home/local-user/.kanvibe/ssh-%C",
      ]));
    } finally {
      consoleErrorSpy.mockRestore();
    }
  });

  it("원격 SSH 명령이 timeout되면 ControlMaster를 종료하고 후속 명령을 cooldown 처리한다", async () => {
    // Given
    const consoleErrorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    mocks.execFile.mockImplementation((_file: string, _args: string[], _options: unknown, callback: (error: null, result: { stdout: string }) => void) => {
      callback(null, { stdout: "" });
    });
    mocks.spawn.mockImplementation(() => {
      const child = createMockSSHProcess();
      queueMicrotask(() => {
        const error = new Error("Command timed out") as Error & { killed?: boolean; signal?: string };
        error.killed = true;
        error.signal = "SIGTERM";
        child.emitError(error);
      });
      return child;
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
      // When & Then
      await expect(execGit("long-running-command", "remote-host", { timeoutMs: 1000 })).rejects.toThrow(/1초/);
      await expect(execGit("next-command", "remote-host")).rejects.toThrow(/최근 SSH transport 실패/);
      expect(mocks.spawn).toHaveBeenCalledTimes(3);
      expect(mocks.execFile).toHaveBeenCalledTimes(3);
      expect(mocks.execFile.mock.calls[0]?.[1]).toEqual(expect.arrayContaining(["-O", "exit"]));
    } finally {
      consoleErrorSpy.mockRestore();
    }
  });

  it("SSH transport 오류는 3회 안에 성공하면 원격 명령 결과를 반환한다", async () => {
    // Given
    const consoleErrorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    mocks.execFile.mockImplementation((_file: string, _args: string[], _options: unknown, callback: (error: null, result: { stdout: string }) => void) => {
      callback(null, { stdout: "" });
    });
    mocks.spawn.mockImplementation(() => {
      const child = createMockSSHProcess();
      const attempt = mocks.spawn.mock.calls.length;
      queueMicrotask(() => {
        if (attempt === 1) {
          child.writeStderr("Connection reset by peer\n");
          child.close(255);
          return;
        }

        completeRemoteCommand(child, "recovered");
      });
      return child;
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
      const result = await execGit("recoverable-command", "remote-host");

      // Then
      expect(result).toBe("recovered");
      expect(mocks.spawn).toHaveBeenCalledTimes(2);
      expect(mocks.execFile).toHaveBeenCalledTimes(1);
      await expect(execGit("next-command", "remote-host")).resolves.toBe("recovered");
    } finally {
      consoleErrorSpy.mockRestore();
    }
  });

  it("같은 SSH host의 원격 명령은 제한된 동시성으로 실행한다", async () => {
    // Given
    const startedCommands: string[] = [];
    const pendingChildren: MockSSHProcess[] = [];
    mocks.spawn.mockImplementation((_file: string, args: string[]) => {
      const child = createMockSSHProcess();
      startedCommands.push(args.at(-1) ?? "");
      pendingChildren.push(child);
      return child;
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
    const third = execGit("third", "remote-host");
    const fourth = execGit("fourth", "remote-host");
    const fifth = execGit("fifth", "remote-host");
    await new Promise((resolve) => setTimeout(resolve, 0));

    // Then
    expect(startedCommands).toHaveLength(4);
    expect(startedCommands[0]).toContain("first");
    expect(startedCommands[1]).toContain("second");
    expect(startedCommands[2]).toContain("third");
    expect(startedCommands[3]).toContain("fourth");

    completeRemoteCommand(pendingChildren.shift()!, "one");
    await expect(first).resolves.toBe("one");
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(startedCommands).toHaveLength(5);
    expect(startedCommands[4]).toContain("fifth");

    completeRemoteCommand(pendingChildren.shift()!, "two");
    await expect(second).resolves.toBe("two");
    completeRemoteCommand(pendingChildren.shift()!, "three");
    completeRemoteCommand(pendingChildren.shift()!, "four");
    completeRemoteCommand(pendingChildren.shift()!, "five");
    await expect(third).resolves.toBe("three");
    await expect(fourth).resolves.toBe("four");
    await expect(fifth).resolves.toBe("five");
  });

  it("환경변수로 같은 SSH host의 원격 명령 동시성 제한을 늘릴 수 있다", async () => {
    // Given
    const originalConcurrency = process.env.KANVIBE_REMOTE_SSH_HOST_MAX_CONCURRENCY;
    process.env.KANVIBE_REMOTE_SSH_HOST_MAX_CONCURRENCY = "6";
    const startedCommands: string[] = [];
    const pendingChildren: MockSSHProcess[] = [];
    mocks.spawn.mockImplementation((_file: string, args: string[]) => {
      const child = createMockSSHProcess();
      startedCommands.push(args.at(-1) ?? "");
      pendingChildren.push(child);
      return child;
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

    try {
      const { execGit } = await import("@/lib/gitOperations");

      // When
      const commands = Array.from({ length: 7 }, (_, index) => execGit(`command-${index}`, "remote-host"));
      await new Promise((resolve) => setTimeout(resolve, 0));

      // Then
      expect(startedCommands).toHaveLength(6);
      for (let index = 0; index < 6; index += 1) {
        expect(startedCommands[index]).toContain(`command-${index}`);
      }

      completeRemoteCommand(pendingChildren.shift()!, "done");
      await expect(commands[0]).resolves.toBe("done");
      await new Promise((resolve) => setTimeout(resolve, 0));

      expect(startedCommands).toHaveLength(7);
      expect(startedCommands[6]).toContain("command-6");
      for (const [index, command] of commands.slice(1).entries()) {
        completeRemoteCommand(pendingChildren.shift()!, String(index));
        await expect(command).resolves.toBe(String(index));
      }
    } finally {
      if (originalConcurrency === undefined) {
        delete process.env.KANVIBE_REMOTE_SSH_HOST_MAX_CONCURRENCY;
      } else {
        process.env.KANVIBE_REMOTE_SSH_HOST_MAX_CONCURRENCY = originalConcurrency;
      }
    }
  });

  it("조용한 probe 명령 실패는 콘솔 에러를 남기지 않는다", async () => {
    // Given
    const consoleErrorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    mocks.spawn.mockImplementation(() => {
      const child = createMockSSHProcess();
      queueMicrotask(() => child.close(255));
      return child;
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
    mocks.execFile.mockImplementation((_file: string, _args: string[], _options: unknown, callback: (error: null, result: { stdout: string }) => void) => {
      callback(null, { stdout: "" });
    });
    mocks.spawn.mockImplementation(() => {
      const child = createMockSSHProcess();
      queueMicrotask(() => {
        child.writeStderr("Connection reset by peer\n");
        child.close(255);
      });
      return child;
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
