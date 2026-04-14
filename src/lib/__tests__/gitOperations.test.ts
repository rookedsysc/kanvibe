import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  homedir: vi.fn(() => "/home/local-user"),
  exec: vi.fn(),
  execFile: vi.fn(),
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
    expect(result).toBe("ok");
    expect(mocks.execFile).toHaveBeenCalledWith(
      "ssh",
      [
        "-i",
        "/tmp/test-key",
        "-p",
        "2202",
        "-o",
        "BatchMode=yes",
        "-o",
        "IdentitiesOnly=yes",
        "-T",
        "remote-host",
        "sh -lc 'pwd'",
      ],
      expect.objectContaining({ maxBuffer: 10 * 1024 * 1024 }),
      expect.any(Function),
    );
  });
});
