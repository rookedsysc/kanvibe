import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  homedir: vi.fn(() => "/home/local-user"),
  availableParallelism: vi.fn(() => 2),
  exec: vi.fn(),
  execFile: vi.fn(),
  spawn: vi.fn(),
  spawnMaster: vi.fn(),
  mkdir: vi.fn(async () => undefined),
}));

vi.mock("child_process", () => ({
  default: {
    exec: mocks.exec,
    execFile: mocks.execFile,
    spawn: (...args: unknown[]) => (args[1] as string[]).includes("-N") ? mocks.spawnMaster(...args) : mocks.spawn(...args),
  },
  exec: mocks.exec,
  execFile: mocks.execFile,
  spawn: (...args: unknown[]) => (args[1] as string[]).includes("-N") ? mocks.spawnMaster(...args) : mocks.spawn(...args),
}));

vi.mock("os", () => ({
  default: {
    homedir: mocks.homedir,
    availableParallelism: mocks.availableParallelism,
  },
  homedir: mocks.homedir,
  availableParallelism: mocks.availableParallelism,
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
  stdin = { on: vi.fn(), end: vi.fn() };
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

function getSpawnedControlPath(callIndex = 0): string {
  return ((getSpawnedSSHArgs(callIndex).find((arg) => arg.startsWith("ControlPath=")) ?? "")
    .replace("ControlPath=", ""));
}

describe("gitOperations interactive remote transaction", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    mocks.spawnMaster.mockImplementation(() => {
      const child = createMockSSHProcess();
      queueMicrotask(() => child.emit("exit", 0));
      return child;
    });
    mocks.availableParallelism.mockReturnValue(2);
    mocks.execFile.mockImplementation((_file: string, _args: string[], _options: unknown, callback: (error: null, result: { stdout: string }) => void) => callback(null, { stdout: "" }));
    vi.doMock("@/lib/sshConfig", async (importOriginal) => ({
      ...(await importOriginal<typeof import("@/lib/sshConfig")>()),
      parseSSHConfig: vi.fn(async () => [{ host: "remote-host", hostname: "example.com", port: 22, username: "tester", privateKeyPath: "/tmp/test-key" }]),
    }));
  });

  it("여러 청크의 출력 크기는 새 청크만 계산한다", async () => {
    const child = createMockSSHProcess();
    const chunk = "한글🙂".repeat(64);
    let measuredCharacters = 0;
    let searchedCharacters = 0;
    mocks.spawn.mockImplementation(() => {
      queueMicrotask(() => {
        const original = Buffer.byteLength;
        const measure = vi.spyOn(Buffer, "byteLength").mockImplementation((value, encoding) => {
          if (typeof value === "string") measuredCharacters += value.length;
          return original(value, encoding);
        });
        const originalSearch = String.prototype.lastIndexOf;
        const search = vi.spyOn(String.prototype, "lastIndexOf").mockImplementation(function (this: string, value, position) {
          if (value === `\n${REMOTE_COMMAND_EXIT_MARKER}:`) searchedCharacters += this.length;
          return originalSearch.call(this, value, position);
        });
        try {
          for (let index = 0; index < 128; index += 1) child.writeStdout(chunk);
          completeRemoteCommand(child, "");
        } finally {
          measure.mockRestore();
          search.mockRestore();
        }
      });
      return child;
    });
    const { execGit } = await import("@/lib/gitOperations");
    await expect(execGit("stream", "remote-host")).resolves.toBe(chunk.repeat(128));
    expect(measuredCharacters).toBeLessThan(chunk.length * 128 * 3);
    expect(searchedCharacters).toBeLessThan(chunk.length * 128 * 3);
  });

  it.each(["success", "maxBuffer"])("%s 확정 후 늦은 출력은 다시 처리하지 않는다", async (outcome) => {
    const child = createMockSSHProcess();
    let lateMeasurements = 0;
    mocks.spawn.mockImplementation(() => {
      queueMicrotask(() => {
        if (outcome === "success") completeRemoteCommand(child, "ok");
        else child.writeStdout("x".repeat(101));
        const measure = vi.spyOn(Buffer, "byteLength");
        try {
          child.writeStdout("late stdout");
          child.writeStderr("late stderr");
          lateMeasurements = measure.mock.calls.length;
        } finally {
          measure.mockRestore();
        }
      });
      return child;
    });
    const { execGit } = await import("@/lib/gitOperations");
    const result = execGit("stream", "remote-host", { maxBufferBytes: 100 });
    if (outcome === "success") await expect(result).resolves.toBe("ok");
    else await expect(result).rejects.toThrow("exceeded maxBuffer");
    expect(lateMeasurements).toBe(0);
    expect(child.kill).toHaveBeenCalledTimes(1);
  });

  it.each([
    { chunks: Array.from(`한글🙂\n${REMOTE_COMMAND_EXIT_MARKER}:0\r\n`), expected: "한글🙂" },
    { chunks: [`before\n${REMOTE_COMMAND_EXIT_MARKER}:0\r`, "", "\n"], expected: "before" },
    { chunks: [`before\n${REMOTE_COMMAND_EXIT_MARKER}:9\nignored\n${REMOTE_COMMAND_EXIT_MARKER}:0\n`], expected: `before\n${REMOTE_COMMAND_EXIT_MARKER}:9\nignored` },
    { chunks: [`before\n${REMOTE_COMMAND_EXIT_MARKER}:0\nlast\n${REMOTE_COMMAND_EXIT_MARKER}:`], expected: `before\n${REMOTE_COMMAND_EXIT_MARKER}:0\nlast\n${REMOTE_COMMAND_EXIT_MARKER}:` },
    { chunks: [`before\n${REMOTE_COMMAND_EXIT_MARKER}:x`, "ignored"], expected: `before\n${REMOTE_COMMAND_EXIT_MARKER}:xignored` },
  ])("완료 마커의 청크 경계와 마지막 prefix 우선순위를 유지한다: $expected", async ({ chunks, expected }) => {
    const child = createMockSSHProcess();
    mocks.spawn.mockImplementation(() => {
      queueMicrotask(() => {
        for (const chunk of chunks) child.writeStdout(chunk);
        child.close(0);
      });
      return child;
    });
    const { execGit } = await import("@/lib/gitOperations");
    await expect(execGit("stream", "remote-host")).resolves.toBe(expected);
  });

  it("UTF-8 stdout와 stderr를 합산하고 정확한 한도까지 허용한다", async () => {
    const child = createMockSSHProcess();
    mocks.spawn.mockImplementation(() => {
      queueMicrotask(() => {
        child.writeStdout("한");
        child.writeStderr("🙂");
        child.writeStdout("글");
        child.close(0);
      });
      return child;
    });
    const { execGit } = await import("@/lib/gitOperations");
    await expect(execGit("stream", "remote-host", { maxBufferBytes: 10 })).resolves.toBe("한글");
  });

  it("청크 경계의 surrogate pair도 합쳐진 UTF-8 크기로 계산한다", async () => {
    const child = createMockSSHProcess();
    mocks.spawn.mockImplementation(() => {
      queueMicrotask(() => {
        child.writeStdout("\ud83d");
        child.writeStdout("");
        child.writeStdout("\ude42");
        child.close(0);
      });
      return child;
    });
    const { execGit } = await import("@/lib/gitOperations");
    await expect(execGit("stream", "remote-host", { maxBufferBytes: 4 })).resolves.toBe("🙂");
  });

  it("responder 요청 행을 제거한 뒤 출력 한도를 다시 사용할 수 있다", async () => {
    const child = createMockSSHProcess();
    const payload = "x".repeat(80);
    const output = "한".repeat(25);
    child.stdin.end.mockImplementation(() => queueMicrotask(() => {
      child.writeStdout(output);
      child.close(0);
    }));
    mocks.spawn.mockImplementation(() => {
      queueMicrotask(() => child.writeStdout(`READY\t${payload}\n`));
      return child;
    });
    const respond = vi.fn(async () => "reply");
    const { execGit } = await import("@/lib/gitOperations");
    await expect(execGit("transaction", "remote-host", { maxBufferBytes: 100, remoteStdinResponder: { marker: "READY", respond } })).resolves.toBe(output);
    expect(respond).toHaveBeenCalledWith(payload);
  });

  it("responder 요청 제거 전에 한도를 넘으면 callback을 실행하지 않는다", async () => {
    const child = createMockSSHProcess();
    mocks.spawn.mockImplementation(() => {
      queueMicrotask(() => child.writeStdout("READY\toversized\n"));
      return child;
    });
    const respond = vi.fn(async () => "reply");
    const { execGit } = await import("@/lib/gitOperations");
    await expect(execGit("transaction", "remote-host", { maxBufferBytes: 10, remoteStdinResponder: { marker: "READY", respond } })).rejects.toThrow("exceeded maxBuffer");
    expect(respond).not.toHaveBeenCalled();
  });

  it("분할 수신한 요청 한 줄에 한 번만 응답하고 비밀 내용은 stdin으로 보낸다", async () => {
    const child = createMockSSHProcess();
    const respond = vi.fn(async (payload: string) => {
      expect(child.stdin.end).not.toHaveBeenCalled();
      expect(payload).toBe("b2xk");
      return "secret-response\n";
    });
    child.stdin.end.mockImplementation(() => queueMicrotask(() => completeRemoteCommand(child, "done")));
    mocks.spawn.mockImplementation(() => {
      queueMicrotask(() => {
        child.writeStdout("login banner\nREADY\tb2");
        child.writeStdout("xk\n");
        child.writeStdout("READY\tb2xk\n");
      });
      return child;
    });
    const { execGit } = await import("@/lib/gitOperations");
    await expect(execGit("transaction", "remote-host", { timeoutMs: 100, remoteStdinResponder: { marker: "READY", respond } })).resolves.toContain("done");
    expect(respond).toHaveBeenCalledTimes(1);
    expect(child.stdin.end).toHaveBeenCalledWith("secret-response\n");
    expect(getSpawnedSSHArgs().join(" ")).not.toContain("secret-response");
  });

  it("한 슬롯에서도 callback의 같은 host 명령이 교착 없이 같은 shard를 쓴다", async () => {
    vi.stubEnv("KANVIBE_REMOTE_SSH_HOST_MAX_CONCURRENCY", "1");
    try {
      const holder = createMockSSHProcess();
      const nested = createMockSSHProcess();
      holder.stdin.end.mockImplementation(() => queueMicrotask(() => completeRemoteCommand(holder, "committed")));
      mocks.spawn.mockImplementation(() => {
        const child = mocks.spawn.mock.calls.length === 1 ? holder : nested;
        queueMicrotask(() => child === holder ? child.writeStdout("READY\t\n") : completeRemoteCommand(child, "nested-result"));
        return child;
      });
      const { execGit } = await import("@/lib/gitOperations");
      const respond = vi.fn(async () => {
        expect(await execGit("herdr workspace list", "remote-host", { timeoutMs: 100 })).toBe("nested-result");
        return "response\n";
      });
      await expect(execGit("transaction", "remote-host", { timeoutMs: 150, remoteStdinResponder: { marker: "READY", respond } })).resolves.toContain("committed");
      expect(respond).toHaveBeenCalledTimes(1);
      expect(mocks.spawn).toHaveBeenCalledTimes(2);
      expect(getSpawnedControlPath(1)).toBe(getSpawnedControlPath(0));
    } finally {
      vi.unstubAllEnvs();
    }
  });

  it("callback 예외는 입력과 SSH 프로세스를 닫고 재실행하지 않는다", async () => {
    const child = createMockSSHProcess();
    mocks.spawn.mockImplementation(() => {
      queueMicrotask(() => child.writeStdout("READY\t\n"));
      return child;
    });
    const respond = vi.fn(async () => { throw new Error("update rejected"); });
    const { execGit } = await import("@/lib/gitOperations");
    await expect(execGit("transaction", "remote-host", { timeoutMs: 100, remoteStdinResponder: { marker: "READY", respond } })).rejects.toThrow("update rejected");
    expect(respond).toHaveBeenCalledTimes(1);
    expect(child.stdin.end).toHaveBeenCalled();
    expect(child.kill).toHaveBeenCalled();
    expect(mocks.spawn).toHaveBeenCalledTimes(1);
  });

  it("응답 후 transport가 끊겨도 callback이나 transaction을 재실행하지 않는다", async () => {
    const child = createMockSSHProcess();
    child.stdin.end.mockImplementation(() => queueMicrotask(() => {
      child.writeStderr("Connection reset by peer");
      child.close(255);
    }));
    mocks.spawn.mockImplementation(() => {
      queueMicrotask(() => child.writeStdout("READY\t\n"));
      return child;
    });
    const respond = vi.fn(async () => "response\n");
    const { execGit } = await import("@/lib/gitOperations");
    await expect(execGit("transaction", "remote-host", { timeoutMs: 100, remoteStdinResponder: { marker: "READY", respond } })).rejects.toThrow("Connection reset by peer");
    expect(respond).toHaveBeenCalledTimes(1);
    expect(mocks.spawn).toHaveBeenCalledTimes(1);
  });

  it("요청 직후 SSH가 종료되면 아직 시작하지 않은 callback을 실행하지 않는다", async () => {
    const child = createMockSSHProcess();
    mocks.spawn.mockImplementation(() => {
      queueMicrotask(() => {
        child.writeStdout("READY\t\n");
        child.writeStderr("Connection reset by peer");
        child.close(255);
      });
      return child;
    });
    const respond = vi.fn(async () => "response\n");
    const { execGit } = await import("@/lib/gitOperations");
    await expect(execGit("transaction", "remote-host", { timeoutMs: 100, remoteStdinResponder: { marker: "READY", respond } })).rejects.toThrow("Connection reset by peer");
    expect(respond).not.toHaveBeenCalled();
  });

  it("transport 오류와 cooldown 오류에 실제 남은 재시도 시간을 첨부한다", async () => {
    vi.stubEnv("KANVIBE_REMOTE_SSH_TRANSPORT_FAILURE_COOLDOWN_MS", "5000");
    const now = vi.spyOn(Date, "now").mockReturnValue(1000);
    try {
      mocks.spawn.mockImplementation(() => {
        const child = createMockSSHProcess();
        queueMicrotask(() => { child.writeStderr("Connection reset by peer"); child.close(255); });
        return child;
      });
      const { execGit } = await import("@/lib/gitOperations");
      await expect(execGit("command", "remote-host")).rejects.toMatchObject({ retryAfterMs: 5000 });
      now.mockReturnValue(2500);
      await expect(execGit("following", "remote-host")).rejects.toMatchObject({ retryAfterMs: 3500 });
      expect(mocks.spawn).toHaveBeenCalledTimes(1);
    } finally {
      now.mockRestore();
      vi.unstubAllEnvs();
    }
  });
});

describe("gitOperations.resolvePathForShell", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    mocks.spawnMaster.mockImplementation(() => {
      const child = createMockSSHProcess();
      queueMicrotask(() => child.emit("exit", 0));
      return child;
    });
    mocks.availableParallelism.mockReturnValue(2);
  });

  it("로컬 틸드 경로는 로컬 홈 디렉토리로 확장한다", async () => {
    // Given
    const { resolvePathForShell } = await import("@/lib/gitOperations");

    // When
    const result = resolvePathForShell("~/work");

    // Then
    expect(result).toBe("'/home/local-user/work'");
  });

  it("원격 틸드 경로는 원격 HOME 기준으로 검색한다", async () => {
    // Given
    const { resolvePathForShell } = await import("@/lib/gitOperations");

    // When
    const result = resolvePathForShell("~/work", "remote-host");

    // Then
    expect(result).toBe("\"$HOME\"'/work'");
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
    const execOptions = mocks.exec.mock.calls[0]?.[1] as { env?: { PATH?: string } };
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
    if (process.platform === "darwin") {
      expect(execOptions.env?.PATH).toContain(".opencode/bin");
    }
  });

  it("표준 입력을 지정하면 파이프로 보내고 명령 인자에는 싣지 않는다", async () => {
    // Given
    const child = createMockSSHProcess();
    mocks.spawn.mockImplementation(() => {
      queueMicrotask(() => completeRemoteCommand(child, "ok"));
      return child;
    });
    vi.doMock("@/lib/sshConfig", async (importOriginal) => ({
      ...(await importOriginal<typeof import("@/lib/sshConfig")>()),
      parseSSHConfig: vi.fn(async () => [{
        host: "remote-host",
        hostname: "example.com",
        port: 22,
        username: "tester",
        privateKeyPath: "/tmp/test-key",
      }]),
    }));
    const { execGit } = await import("@/lib/gitOperations");

    // When
    await execGit("cat > /tmp/target", "remote-host", { remoteStdin: "secret-body" });

    // Then
    expect(child.stdin.end).toHaveBeenCalledWith("secret-body");
    expect(getSpawnedSSHArgs().join(" ")).not.toContain("secret-body");
  });

  it("호스트 키 검증을 요구한 원격 명령은 기존 SSH 연결을 재사용하지 않는다", async () => {
    const child = createMockSSHProcess();
    mocks.spawn.mockImplementation(() => {
      queueMicrotask(() => completeRemoteCommand(child, "ok"));
      return child;
    });
    vi.doMock("@/lib/sshConfig", async (importOriginal) => ({
      ...(await importOriginal<typeof import("@/lib/sshConfig")>()),
      parseSSHConfig: vi.fn(async () => [{
        host: "remote-host",
        hostname: "example.com",
        port: 22,
        username: "tester",
        privateKeyPath: "/tmp/test-key",
      }]),
    }));
    const { execGit } = await import("@/lib/gitOperations");

    await execGit("printf ok", "remote-host", { strictHostKeyChecking: true });

    expect(getSpawnedSSHArgs()).toContain("StrictHostKeyChecking=yes");
    expect(getSpawnedSSHArgs()).toContain("ControlMaster=no");
    expect(getSpawnedSSHArgs()).toContain("ControlPath=none");
    expect(getSpawnedSSHArgs()).not.toContain("ControlMaster=auto");
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
        "ControlMaster=no",
        "-o",
        "ControlPersist=10m",
        "-o",
        expect.stringMatching(/^ControlPath=\/home\/local-user\/\.kanvibe\/ssh-[a-f0-9]{12}-[a-f0-9]{20}-0$/),
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

  it("원격 명령별 출력 한도를 적용한다", async () => {
    mocks.spawn.mockImplementation(() => {
      const child = createMockSSHProcess();
      queueMicrotask(() => completeRemoteCommand(child, "output larger than the command limit"));
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

    await expect(execGit("cat chat.jsonl", "remote-host", { maxBufferBytes: 16 }))
      .rejects.toThrow("exceeded maxBuffer");
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
      expect(mocks.spawn).toHaveBeenCalledTimes(1);
      expect(mocks.spawnMaster).toHaveBeenCalledTimes(1);
      expect(mocks.spawnMaster.mock.calls[0]?.[1]).toEqual(expect.arrayContaining(["-N", "-f"]));
      expect(mocks.spawnMaster.mock.calls.some((call) => call[1].includes("exit"))).toBe(false);
    } finally {
      consoleErrorSpy.mockRestore();
    }
  });

  it("원격 SSH 명령 timeout은 재실행하거나 master를 종료하지 않고 다음 명령을 허용한다", async () => {
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
      await expect(execGit("next-command", "remote-host")).rejects.toThrow(/완료되지 않았/);
      expect(mocks.spawn).toHaveBeenCalledTimes(2);
      expect(mocks.spawnMaster).toHaveBeenCalledTimes(1);
      expect(mocks.spawnMaster.mock.calls.some((call) => call[1].includes("exit"))).toBe(false);
    } finally {
      consoleErrorSpy.mockRestore();
    }
  });

  it("cooldown을 해제하면 같은 SSH host의 다음 명령이 다시 SSH 연결을 시도한다", async () => {
    // Given
    const consoleErrorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    mocks.execFile.mockImplementation((_file: string, _args: string[], _options: unknown, callback: (error: null, result: { stdout: string }) => void) => {
      callback(null, { stdout: "" });
    });
    let isTransportDown = true;
    mocks.spawn.mockImplementation(() => {
      const child = createMockSSHProcess();
      queueMicrotask(() => {
        if (isTransportDown) {
          child.writeStderr("Connection reset by 100.99.109.53 port 22\n");
          child.close(255);
          return;
        }

        completeRemoteCommand(child, "attached");
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

    const { execGit, clearRemoteSSHTransportCooldown } = await import("@/lib/gitOperations");

    try {
      await expect(execGit("background-sync", "remote-host")).rejects.toThrow(/Connection reset/);
      await expect(execGit("blocked-command", "remote-host")).rejects.toThrow(/최근 SSH transport 실패/);
      const spawnCountBeforeClear = mocks.spawn.mock.calls.length;
      isTransportDown = false;

      // When
      clearRemoteSSHTransportCooldown("remote-host");

      // Then
      await expect(execGit("terminal-attach", "remote-host")).resolves.toBe("attached");
      expect(mocks.spawn).toHaveBeenCalledTimes(spawnCountBeforeClear + 1);
      expect(getSpawnedRemoteCommand(spawnCountBeforeClear)).toContain("terminal-attach");
    } finally {
      consoleErrorSpy.mockRestore();
    }
  });

  it("실패를 기록하지 않기로 한 원격 명령은 실패해도 오류 로그를 남기지 않는다", async () => {
    // Given
    const consoleErrorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    mocks.spawn.mockImplementation(() => {
      const child = createMockSSHProcess();
      queueMicrotask(() => {
        child.writeStderr("can't find session: kanvibe-dev\n");
        completeRemoteCommand(child, "", 1);
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
      await expect(execGit("tmux list-windows -t 'kanvibe-dev'", "remote-host", { shouldLogFailure: false })).rejects.toThrow();
      const loggedCountAfterQuietCommand = consoleErrorSpy.mock.calls.length;
      await expect(execGit("tmux list-windows -t 'kanvibe-dev'", "remote-host")).rejects.toThrow();

      // Then
      expect(loggedCountAfterQuietCommand).toBe(0);
      expect(consoleErrorSpy).toHaveBeenCalledWith("[remote-ssh] command failed", expect.objectContaining({ sshHost: "remote-host" }));
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
      const result = await execGit("recoverable-command", "remote-host", { retrySafe: true });

      // Then
      expect(result).toBe("recovered");
      expect(mocks.spawn).toHaveBeenCalledTimes(2);
      expect(mocks.spawnMaster).toHaveBeenCalledTimes(2);
      await expect(execGit("next-command", "remote-host")).resolves.toBe("recovered");
    } finally {
      consoleErrorSpy.mockRestore();
    }
  });

  it("같은 SSH host의 원격 명령은 제한된 동시성으로 실행한다", async () => {
    // Given
    const originalConcurrency = process.env.KANVIBE_REMOTE_SSH_HOST_MAX_CONCURRENCY;
    process.env.KANVIBE_REMOTE_SSH_HOST_MAX_CONCURRENCY = "4";
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
    } finally {
      if (originalConcurrency === undefined) {
        delete process.env.KANVIBE_REMOTE_SSH_HOST_MAX_CONCURRENCY;
      } else {
        process.env.KANVIBE_REMOTE_SSH_HOST_MAX_CONCURRENCY = originalConcurrency;
      }
    }
  });

  it("기본 SSH host 동시성은 CPU와 무관하게 여덟 채널로 제한한다", async () => {
    // Given
    const originalConcurrency = process.env.KANVIBE_REMOTE_SSH_HOST_MAX_CONCURRENCY;
    delete process.env.KANVIBE_REMOTE_SSH_HOST_MAX_CONCURRENCY;
    mocks.availableParallelism.mockReturnValue(3);
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
      const commands = Array.from({ length: 9 }, (_, index) => execGit(`command-${index}`, "remote-host"));
      await new Promise((resolve) => setTimeout(resolve, 0));

      // Then
      expect(startedCommands).toHaveLength(8);
      for (let index = 0; index < 8; index += 1) {
        expect(startedCommands[index]).toContain(`command-${index}`);
      }

      completeRemoteCommand(pendingChildren.shift()!, "done");
      await expect(commands[0]).resolves.toBe("done");
      await new Promise((resolve) => setTimeout(resolve, 0));

      expect(startedCommands).toHaveLength(9);
      expect(startedCommands[8]).toContain("command-8");
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

  it("환경변수 동시성도 여덟 채널을 넘지 않는다", async () => {
    // Given
    const originalConcurrency = process.env.KANVIBE_REMOTE_SSH_HOST_MAX_CONCURRENCY;
    process.env.KANVIBE_REMOTE_SSH_HOST_MAX_CONCURRENCY = "12";
    mocks.availableParallelism.mockReturnValue(2);
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
      const commands = Array.from({ length: 9 }, (_, index) => execGit(`command-${index}`, "remote-host"));
      await new Promise((resolve) => setTimeout(resolve, 0));

      // Then
      expect(startedCommands).toHaveLength(8);
      for (let index = 0; index < 8; index += 1) {
        expect(startedCommands[index]).toContain(`command-${index}`);
      }

      completeRemoteCommand(pendingChildren.shift()!, "done");
      await expect(commands[0]).resolves.toBe("done");
      await new Promise((resolve) => setTimeout(resolve, 0));

      expect(startedCommands).toHaveLength(9);
      expect(startedCommands[8]).toContain("command-8");
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

  it("동시 원격 명령은 같은 process-owned ControlMaster를 공유한다", async () => {
    if (process.platform === "win32") {
      return;
    }

    // Given
    const originalConcurrency = process.env.KANVIBE_REMOTE_SSH_HOST_MAX_CONCURRENCY;
    process.env.KANVIBE_REMOTE_SSH_HOST_MAX_CONCURRENCY = "6";
    const pendingChildren: MockSSHProcess[] = [];
    mocks.spawn.mockImplementation(() => {
      const child = createMockSSHProcess();
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
      const commands = Array.from({ length: 6 }, (_, index) => execGit(`command-${index}`, "remote-host"));
      await new Promise((resolve) => setTimeout(resolve, 0));

      // Then
      const controlPaths = mocks.spawn.mock.calls.map((_, index) => getSpawnedControlPath(index));
      expect(new Set(controlPaths).size).toBe(1);
      expect(controlPaths[0]).toMatch(/^\/home\/local-user\/\.kanvibe\/ssh-[a-f0-9]{12}-[a-f0-9]{20}-0$/);
      expect(mocks.spawnMaster).toHaveBeenCalledTimes(1);

      for (const [index, command] of commands.entries()) {
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

  it("완료된 command의 master를 후속 command가 계속 공유한다", async () => {
    if (process.platform === "win32") {
      return;
    }

    // Given
    const originalConcurrency = process.env.KANVIBE_REMOTE_SSH_HOST_MAX_CONCURRENCY;
    process.env.KANVIBE_REMOTE_SSH_HOST_MAX_CONCURRENCY = "2";
    const pendingChildren: MockSSHProcess[] = [];
    mocks.spawn.mockImplementation(() => {
      const child = createMockSSHProcess();
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
      const first = execGit("first", "remote-host");
      const second = execGit("second", "remote-host");
      const third = execGit("third", "remote-host");
      await new Promise((resolve) => setTimeout(resolve, 0));

      // Then
      expect(mocks.spawn).toHaveBeenCalledTimes(2);
      const firstControlPath = getSpawnedControlPath(0);
      expect(firstControlPath).toMatch(/^\/home\/local-user\/\.kanvibe\/ssh-[a-f0-9]{12}-[a-f0-9]{20}-0$/);
      expect(getSpawnedControlPath(1)).toBe(firstControlPath);

      completeRemoteCommand(pendingChildren.shift()!, "one");
      await expect(first).resolves.toBe("one");
      await new Promise((resolve) => setTimeout(resolve, 0));

      expect(mocks.spawn).toHaveBeenCalledTimes(3);
      expect(getSpawnedControlPath(2)).toBe(firstControlPath);

      completeRemoteCommand(pendingChildren.shift()!, "two");
      completeRemoteCommand(pendingChildren.shift()!, "three");
      await expect(second).resolves.toBe("two");
      await expect(third).resolves.toBe("three");
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
      "find '/workspace' -maxdepth 4 -name \".git\" \\( -type d -o -type f \\) 2>/dev/null",
      expect.objectContaining({ env: expect.any(Object) }),
      expect.any(Function),
    );
    expect(result).toEqual(["/workspace/api", "/workspace/feature-worktree"]);
  });
});

describe("SSH connection ownership and execution safety", () => {
  const config = { host: "remote-host", hostname: "example.com", port: 22, username: "tester", privateKeyPath: "/tmp/test-key" };
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    mocks.spawnMaster.mockImplementation(() => {
      const child = createMockSSHProcess();
      queueMicrotask(() => child.emit("exit", 0));
      return child;
    });
    vi.unstubAllEnvs();
    vi.doMock("@/lib/sshConfig", async (importOriginal) => ({
      ...(await importOriginal<typeof import("@/lib/sshConfig")>()),
      parseSSHConfig: vi.fn(async () => [config]),
    }));
  });

  it.each(["Permission denied (publickey)", "Host key verification failed", "REMOTE HOST IDENTIFICATION HAS CHANGED"])("permanent failure %s is not transient and is never replayed", async (message) => {
    mocks.spawn.mockImplementation(() => {
      const child = createMockSSHProcess();
      queueMicrotask(() => { child.writeStderr(message); child.close(255); });
      return child;
    });
    const { execGit, isSSHTransportError } = await import("@/lib/gitOperations");
    expect(isSSHTransportError(new Error(message))).toBe(false);
    await expect(execGit("cat config", "remote-host", { retrySafe: true })).rejects.toThrow(message);
    expect(mocks.spawn).toHaveBeenCalledTimes(1);
    await expect(execGit("cat config", "remote-host", { retrySafe: true })).rejects.toThrow(message);
    expect(mocks.spawn).toHaveBeenCalledTimes(2);
    expect(mocks.spawnMaster.mock.calls.some((call) => call[1].includes("exit"))).toBe(false);
  });

  it("does not replay a mutation when its result is unknown", async () => {
    mocks.spawn.mockImplementation(() => {
      const child = createMockSSHProcess();
      queueMicrotask(() => { child.writeStderr("Connection reset by peer"); child.close(255); });
      return child;
    });
    const { execGit } = await import("@/lib/gitOperations");
    await expect(execGit("herdr workspace create", "remote-host")).rejects.toThrow("Connection reset");
    expect(mocks.spawn).toHaveBeenCalledTimes(1);
    expect(mocks.spawnMaster.mock.calls.some((call) => call[1].includes("exit"))).toBe(false);
  });

  it("background saturation reserves capacity for interactive detail reads", async () => {
    vi.stubEnv("KANVIBE_REMOTE_SSH_HOST_MAX_CONCURRENCY", "3");
    const children: MockSSHProcess[] = [];
    mocks.spawn.mockImplementation(() => {
      const child = createMockSSHProcess();
      children.push(child);
      return child;
    });
    const { execGit, runWithBackgroundSSHCommands } = await import("@/lib/gitOperations");
    const background = Array.from({ length: 4 }, (_, i) => runWithBackgroundSSHCommands(() => execGit(`repair-${i}`, "remote-host")));
    try {
      await vi.waitFor(() => expect(children).toHaveLength(2));
      const detail = execGit("interactive-detail", "remote-host");
      await vi.waitFor(() => expect(children).toHaveLength(3));
      expect(getSpawnedRemoteCommand(2)).toContain("interactive-detail");
      completeRemoteCommand(children[2], "usable");
      await expect(detail).resolves.toBe("usable");
      expect(children).toHaveLength(3);
      completeRemoteCommand(children[0], "repaired");
      await background[0];
      await vi.waitFor(() => expect(children).toHaveLength(4));
      completeRemoteCommand(children[1], "repaired");
      await background[1];
      await vi.waitFor(() => expect(children).toHaveLength(5));
      completeRemoteCommand(children[3], "repaired");
      completeRemoteCommand(children[4], "repaired");
      await Promise.all(background);
    } finally {
      for (const child of children) completeRemoteCommand(child, "cleanup");
      vi.unstubAllEnvs();
    }
  });

  it("interactive queued commands precede background work without raising the host ceiling", async () => {
    vi.stubEnv("KANVIBE_REMOTE_SSH_HOST_MAX_CONCURRENCY", "1");
    const children: MockSSHProcess[] = [];
    mocks.spawn.mockImplementation(() => {
      const child = createMockSSHProcess();
      children.push(child);
      return child;
    });
    const { execGit, runWithBackgroundSSHCommands } = await import("@/lib/gitOperations");
    try {
      const active = execGit("active", "remote-host");
      await vi.waitFor(() => expect(children).toHaveLength(1));
      const background = runWithBackgroundSSHCommands(() => execGit("repair", "remote-host"));
      const detail = execGit("interactive-detail", "remote-host");
      completeRemoteCommand(children[0], "done");
      await active;
      await vi.waitFor(() => expect(children).toHaveLength(2));
      expect(getSpawnedRemoteCommand(1)).toContain("interactive-detail");
      completeRemoteCommand(children[1], "usable");
      await detail;
      await vi.waitFor(() => expect(children).toHaveLength(3));
      completeRemoteCommand(children[2], "repaired");
      await background;
    } finally {
      for (const child of children) completeRemoteCommand(child, "cleanup");
      vi.unstubAllEnvs();
    }
  });

  it("queued expiry never dispatches after capacity is released", async () => {
    vi.stubEnv("KANVIBE_REMOTE_SSH_HOST_MAX_CONCURRENCY", "1");
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "Date"] });
    const activeChild = createMockSSHProcess();
    mocks.spawn.mockImplementation(() => {
      if (mocks.spawn.mock.calls.length === 1) return activeChild;
      const next = createMockSSHProcess();
      queueMicrotask(() => completeRemoteCommand(next, "next"));
      return next;
    });
    const { execGit } = await import("@/lib/gitOperations");
    const active = execGit("active", "remote-host", { timeoutMs: 500 });
    const activeResult = active.catch((error: unknown) => error);
    const expired = execGit("expired", "remote-host", { timeoutMs: 20 });
    const failure = vi.fn();
    const expiredResult = expired.catch(failure);
    try {
      await vi.advanceTimersByTimeAsync(1);
      await vi.advanceTimersByTimeAsync(25);
      expect(failure).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ message: expect.stringMatching(/SSH 명령이 .*완료되지 않았/) }));
      expect(mocks.spawn).toHaveBeenCalledTimes(1);
      completeRemoteCommand(activeChild, "done");
      await expect(activeResult).resolves.toBe("done");
      await expiredResult;
      await expect(execGit("next", "remote-host", { timeoutMs: 100 })).resolves.toBe("next");
      expect(mocks.spawn).toHaveBeenCalledTimes(2);
      expect(getSpawnedRemoteCommand(1)).toContain("next");
    } finally {
      completeRemoteCommand(activeChild, "cleanup");
      await Promise.all([activeResult, expiredResult]);
      vi.useRealTimers();
      vi.unstubAllEnvs();
    }
  });

  it("a saturated repair host does not block another host", async () => {
    vi.stubEnv("KANVIBE_REMOTE_SSH_HOST_MAX_CONCURRENCY", "1");
    vi.doMock("@/lib/sshConfig", async (importOriginal) => ({
      ...(await importOriginal<typeof import("@/lib/sshConfig")>()),
      parseSSHConfig: vi.fn(async () => [config, { ...config, host: "other-host", hostname: "other.example.com" }]),
    }));
    const active = createMockSSHProcess();
    mocks.spawn.mockImplementation((_file: string, args: string[]) => {
      if (args.includes("remote-host")) return active;
      const child = createMockSSHProcess();
      queueMicrotask(() => completeRemoteCommand(child, "other-host-ready"));
      return child;
    });
    const { execGit, runWithBackgroundSSHCommands } = await import("@/lib/gitOperations");
    const repair = runWithBackgroundSSHCommands(() => execGit("repair", "remote-host"));
    try {
      await vi.waitFor(() => expect(mocks.spawn).toHaveBeenCalledOnce());
      await expect(execGit("detail", "other-host")).resolves.toBe("other-host-ready");
      completeRemoteCommand(active, "repaired");
      await expect(repair).resolves.toBe("repaired");
    } finally {
      completeRemoteCommand(active, "cleanup");
      vi.unstubAllEnvs();
    }
  });

  it("a command timeout preserves a concurrent command and allows the next command", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "Date"] });
    const timedOut = createMockSSHProcess();
    const concurrent = createMockSSHProcess();
    mocks.spawn.mockImplementationOnce(() => timedOut).mockImplementationOnce(() => concurrent).mockImplementation(() => {
      const child = createMockSSHProcess();
      queueMicrotask(() => completeRemoteCommand(child, "next"));
      return child;
    });
    const { execGit } = await import("@/lib/gitOperations");
    try {
      const first = execGit("mutation", "remote-host", { timeoutMs: 25 });
      const firstResult = first.catch((error: unknown) => error);
      const second = execGit("working", "remote-host", { timeoutMs: 500 });
      await vi.advanceTimersByTimeAsync(30);
      expect(await firstResult).toMatchObject({ message: expect.stringMatching(/완료되지 않았/) });
      completeRemoteCommand(concurrent, "survived");
      await expect(second).resolves.toBe("survived");
      await expect(execGit("next", "remote-host")).resolves.toBe("next");
      expect(mocks.spawn).toHaveBeenCalledTimes(3);
      expect(mocks.spawnMaster.mock.calls.some((call) => call[1].includes("exit"))).toBe(false);
    } finally {
      vi.useRealTimers();
    }
  });

  it("joins cold master preparation and shares one connection across concurrent commands", async () => {
    const prepares: Array<() => void> = [];
    mocks.spawnMaster.mockImplementation(() => {
      const child = createMockSSHProcess();
      prepares.push(() => child.emit("exit", 0));
      return child;
    });
    mocks.spawn.mockImplementation(() => {
      const child = createMockSSHProcess();
      queueMicrotask(() => completeRemoteCommand(child, "done"));
      return child;
    });
    const { execGit } = await import("@/lib/gitOperations");
    const operations = [execGit("one", "remote-host"), execGit("two", "remote-host"), execGit("three", "remote-host")];
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(prepares).toHaveLength(1);
    expect(mocks.spawn).not.toHaveBeenCalled();
    prepares[0]();
    await expect(Promise.all(operations)).resolves.toEqual(["done", "done", "done"]);
    const paths = mocks.spawn.mock.calls.map((_, i) => getSpawnedControlPath(i));
    expect(new Set(paths).size).toBe(1);
    expect(getSpawnedSSHArgs()).toContain("ProxyCommand=false");
    expect(getSpawnedSSHArgs()).toContain("ControlMaster=no");
  });

  it("strict verification connections are independent and admitted one at a time", async () => {
    const children: MockSSHProcess[] = [];
    mocks.spawn.mockImplementation(() => { const child = createMockSSHProcess(); children.push(child); return child; });
    const { execGit } = await import("@/lib/gitOperations");
    const one = execGit("read one", "remote-host", { strictHostKeyChecking: true });
    const two = execGit("read two", "remote-host", { strictHostKeyChecking: true });
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(children).toHaveLength(1);
    completeRemoteCommand(children[0], "one");
    await expect(one).resolves.toBe("one");
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(children).toHaveLength(2);
    expect(getSpawnedControlPath(1)).toBe("none");
    completeRemoteCommand(children[1], "two");
    await expect(two).resolves.toBe("two");
    expect(mocks.execFile).not.toHaveBeenCalled();
  });

  it("a completed remote application error is not a transport failure or replay candidate", async () => {
    mocks.spawn.mockImplementation(() => {
      const child = createMockSSHProcess();
      queueMicrotask(() => { child.writeStderr("Connection reset by peer"); completeRemoteCommand(child, "", 1); });
      return child;
    });
    const { execGit } = await import("@/lib/gitOperations");
    await expect(execGit("application", "remote-host", { retrySafe: true })).rejects.toThrow("Connection reset");
    await expect(execGit("application", "remote-host", { retrySafe: true })).rejects.toThrow("Connection reset");
    expect(mocks.spawn).toHaveBeenCalledTimes(2);
  });
  it("bounds a cold caller's deadline without cancelling shared master preparation", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "Date"] });
    const master = createMockSSHProcess();
    mocks.spawnMaster.mockReturnValue(master);
    mocks.spawn.mockImplementation(() => {
      const child = createMockSSHProcess();
      queueMicrotask(() => completeRemoteCommand(child, "healthy"));
      return child;
    });
    const { execGit } = await import("@/lib/gitOperations");
    try {
      const short = execGit("short", "remote-host", { timeoutMs: 20 });
      const expired = vi.fn();
      const shortResult = short.catch(expired);
      const long = execGit("long", "remote-host", { timeoutMs: 500 });
      await vi.advanceTimersByTimeAsync(25);
      expect(expired).toHaveBeenCalledTimes(1);
      expect(expired.mock.calls[0][0].message).toMatch(/완료되지 않았/);
      await shortResult;
      expect(mocks.spawn).not.toHaveBeenCalled();
      master.emit("exit", 0);
      await expect(long).resolves.toBe("healthy");
      await expect(execGit("following", "remote-host")).resolves.toBe("healthy");
      expect(mocks.spawnMaster).toHaveBeenCalledTimes(1);
      expect(mocks.spawn).toHaveBeenCalledTimes(2);
    } finally {
      master.emit("exit", 0);
      vi.useRealTimers();
    }
  });

  it("cold nested commands in an authenticated strict responder cannot deadlock their own admission", async () => {
    vi.stubEnv("KANVIBE_REMOTE_SSH_HOST_MAX_CONCURRENCY", "1");
    const outer = createMockSSHProcess();
    const inner = createMockSSHProcess();
    outer.stdin.end.mockImplementation(() => queueMicrotask(() => completeRemoteCommand(outer, "committed")));
    mocks.spawn.mockImplementationOnce(() => {
      queueMicrotask(() => outer.writeStdout("READY\t\n"));
      return outer;
    }).mockImplementationOnce(() => {
      queueMicrotask(() => completeRemoteCommand(inner, "read"));
      return inner;
    });
    const { execGit } = await import("@/lib/gitOperations");
    const respond = vi.fn(async () => {
      expect(await execGit("read", "remote-host", { timeoutMs: 100 })).toBe("read");
      return "response\n";
    });
    await expect(execGit("transaction", "remote-host", {
      strictHostKeyChecking: true, timeoutMs: 200, remoteStdinResponder: { marker: "READY", respond },
    })).resolves.toBe("committed");
    expect(respond).toHaveBeenCalledTimes(1);
    expect(mocks.spawnMaster).toHaveBeenCalledTimes(1);
    expect(getSpawnedControlPath(0)).toBe("none");
    expect(getSpawnedControlPath(1)).not.toBe("none");
  });

  it("a stale preparation failure cannot poison a newer recovery shared by another caller", async () => {
    mocks.spawnMaster.mockImplementationOnce(() => {
      const child = createMockSSHProcess();
      queueMicrotask(() => { child.writeStderr("Connection reset by peer"); child.close(255); });
      return child;
    });
    mocks.spawn.mockImplementation(() => {
      const child = createMockSSHProcess();
      queueMicrotask(() => completeRemoteCommand(child, "recovered"));
      return child;
    });
    const { execGit } = await import("@/lib/gitOperations");
    const read = execGit("read", "remote-host", { retrySafe: true });
    const write = execGit("mutation", "remote-host");
    const failure = expect(write).rejects.toThrow("Connection reset");
    await expect(read).resolves.toBe("recovered");
    await failure;
    await expect(execGit("following", "remote-host")).resolves.toBe("recovered");
    expect(mocks.spawnMaster).toHaveBeenCalledTimes(2);
    expect(mocks.spawn).toHaveBeenCalledTimes(2);
  });

  it("a permanent master authentication failure never dispatches or replays the remote command", async () => {
    mocks.spawnMaster.mockImplementation(() => {
      const child = createMockSSHProcess();
      queueMicrotask(() => { child.writeStderr("Permission denied (publickey)"); child.close(255); });
      return child;
    });
    const { execGit } = await import("@/lib/gitOperations");
    await expect(execGit("mutation", "remote-host", { retrySafe: true })).rejects.toThrow("Permission denied");
    await expect(execGit("following", "remote-host")).rejects.toThrow("Permission denied");
    expect(mocks.spawnMaster).toHaveBeenCalledTimes(2);
    expect(mocks.spawn).not.toHaveBeenCalled();
  });

  it("expires strict callers while queued for admission and never dispatches their expired command", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "Date"] });
    const active = createMockSSHProcess();
    mocks.spawn.mockReturnValue(active);
    const { execGit } = await import("@/lib/gitOperations");
    const first = execGit("active", "remote-host", { strictHostKeyChecking: true, timeoutMs: 500 });
    const firstResult = first.catch((error: unknown) => error);
    const second = execGit("expired", "remote-host", { strictHostKeyChecking: true, timeoutMs: 20 });
    const failed = vi.fn();
    const secondResult = second.catch(failed);
    try {
      await vi.advanceTimersByTimeAsync(25);
      expect(failed).toHaveBeenCalledTimes(1);
      expect(failed.mock.calls[0][0].message).toMatch(/완료되지 않았/);
      completeRemoteCommand(active, "done");
      await expect(firstResult).resolves.toBe("done");
      await secondResult;
      expect(mocks.spawn).toHaveBeenCalledTimes(1);
    } finally {
      completeRemoteCommand(active, "done");
      await Promise.all([firstResult, secondResult]);
      vi.useRealTimers();
    }
  });

  it("a strict responder releases authentication admission before joining an already queued master", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "Date"] });
    const outer = createMockSSHProcess();
    outer.stdin.end.mockImplementation(() => queueMicrotask(() => completeRemoteCommand(outer, "committed")));
    mocks.spawn.mockImplementationOnce(() => outer).mockImplementation(() => {
      const child = createMockSSHProcess();
      queueMicrotask(() => completeRemoteCommand(child, "read"));
      return child;
    });
    const { execGit } = await import("@/lib/gitOperations");
    const respond = vi.fn(async () => {
      expect(await execGit("nested read", "remote-host", { timeoutMs: 300 })).toBe("read");
      return "response\n";
    });
    const transaction = execGit("transaction", "remote-host", {
      strictHostKeyChecking: true, timeoutMs: 200, remoteStdinResponder: { marker: "READY", respond },
    });
    const transactionResult = transaction.catch((error: unknown) => error);
    await vi.advanceTimersByTimeAsync(0);
    const concurrent = execGit("background read", "remote-host", { timeoutMs: 500 });
    const concurrentResult = concurrent.catch((error: unknown) => error);
    try {
      await vi.advanceTimersByTimeAsync(0);
      expect(mocks.spawnMaster).not.toHaveBeenCalled();
      outer.writeStdout("READY\t\n");
      await vi.advanceTimersByTimeAsync(1);
      const [result, background] = await Promise.all([transactionResult, concurrentResult]);
      expect(result).toBe("committed");
      expect(background).toBe("read");
      expect(respond).toHaveBeenCalledTimes(1);
      expect(mocks.spawnMaster).toHaveBeenCalledTimes(1);
    } finally {
      await vi.advanceTimersByTimeAsync(600);
      await Promise.all([transactionResult, concurrentResult]);
      vi.useRealTimers();
    }
  });

  it("a master preparation failure after every caller left does not trap the next request in a rejected cache entry", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "Date"] });
    const abandoned = createMockSSHProcess();
    mocks.spawnMaster.mockImplementationOnce(() => abandoned);
    mocks.spawn.mockImplementation(() => {
      const child = createMockSSHProcess();
      queueMicrotask(() => completeRemoteCommand(child, "fresh"));
      return child;
    });
    const { execGit } = await import("@/lib/gitOperations");
    try {
      const caller = execGit("expired", "remote-host", { timeoutMs: 20 });
      const callerResult = caller.catch((error: unknown) => error);
      await vi.advanceTimersByTimeAsync(25);
      expect(await callerResult).toMatchObject({ message: expect.stringMatching(/완료되지 않았/) });
      abandoned.writeStderr("Permission denied (publickey)");
      abandoned.close(255);
      await vi.advanceTimersByTimeAsync(0);
      await expect(execGit("fresh", "remote-host")).resolves.toBe("fresh");
      expect(mocks.spawnMaster).toHaveBeenCalledTimes(2);
      expect(mocks.spawn).toHaveBeenCalledTimes(1);
    } finally {
      vi.useRealTimers();
    }
  });

});
