import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  readClaudeAccessToken,
  readClaudeKeychainCredentials,
} from "@/lib/aiUsage/claudeCredentials";

const { mockExecFile } = vi.hoisted(() => ({ mockExecFile: vi.fn() }));

vi.mock("node:child_process", async (importOriginal) => {
  const actual = await importOriginal<typeof import("node:child_process")>();
  return {
    ...actual,
    default: { ...actual, execFile: (...args: unknown[]) => mockExecFile(...args) },
    execFile: (...args: unknown[]) => mockExecFile(...args),
  };
});

const originalPlatform = process.platform;

function stubPlatform(platform: string): void {
  Object.defineProperty(process, "platform", { value: platform, configurable: true });
}

/** execFile은 콜백 방식이라 테스트가 성공/실패를 직접 흉내 낸다 */
function resolveExecFileWith(error: (Error & { code?: number }) | null, stdout: string): void {
  mockExecFile.mockImplementation((_command, _args, _options, callback) => {
    callback(error, stdout, "");
  });
}

function createSecurityFailure(exitCode: number): Error & { code: number } {
  return Object.assign(new Error(`security exited with ${exitCode}`), { code: exitCode });
}

function createCredentialsJson(accessToken: string): string {
  return JSON.stringify({ claudeAiOauth: { accessToken, refreshToken: "refresh-token" } });
}

describe("readClaudeAccessToken", () => {
  it("자격증명 JSON에서 액세스 토큰을 꺼낸다", () => {
    expect(readClaudeAccessToken(createCredentialsJson("access-token"))).toBe("access-token");
  });

  it("빈 문자열·깨진 JSON·토큰 없는 자격증명은 모두 null이다", () => {
    expect(readClaudeAccessToken("")).toBeNull();
    expect(readClaudeAccessToken("{not json")).toBeNull();
    expect(readClaudeAccessToken(JSON.stringify({ claudeAiOauth: {} }))).toBeNull();
    expect(readClaudeAccessToken(JSON.stringify({ claudeAiOauth: { accessToken: "  " } }))).toBeNull();
  });
});

describe("readClaudeKeychainCredentials", () => {
  beforeEach(() => {
    stubPlatform("darwin");
  });

  afterEach(() => {
    stubPlatform(originalPlatform);
    vi.resetAllMocks();
  });

  it("macOS가 아니면 Keychain을 건드리지 않는다", async () => {
    stubPlatform("linux");

    expect(await readClaudeKeychainCredentials()).toEqual({ outcome: "absent" });
    expect(mockExecFile).not.toHaveBeenCalled();
  });

  it("Claude Code가 쓰는 서비스 이름으로 현재 사용자 항목을 조회한다", async () => {
    resolveExecFileWith(null, createCredentialsJson("keychain-token"));

    await readClaudeKeychainCredentials();

    const [command, args] = mockExecFile.mock.calls[0];
    expect(command).toBe("security");
    expect(args).toEqual([
      "find-generic-password",
      "-a",
      expect.any(String),
      "-s",
      "Claude Code-credentials",
      "-w",
    ]);
  });

  it("config dir이 있으면 Claude Code 2.1+의 scoped 서비스를 먼저 조회한다", async () => {
    resolveExecFileWith(null, createCredentialsJson("scoped-token"));

    await readClaudeKeychainCredentials("/Users/tester/.claude");

    const [, args] = mockExecFile.mock.calls[0];
    expect(args).toContain("Claude Code-credentials-ee16a9f4");
    expect(mockExecFile).toHaveBeenCalledTimes(1);
  });

  it("scoped 항목이 없으면 legacy 서비스의 자격증명으로 폴백한다", async () => {
    mockExecFile
      .mockImplementationOnce((_command, _args, _options, callback) => {
        callback(createSecurityFailure(44), "", "");
      })
      .mockImplementationOnce((_command, _args, _options, callback) => {
        callback(null, createCredentialsJson("legacy-token"), "");
      });

    const result = await readClaudeKeychainCredentials("/Users/tester/.claude");

    expect(result.outcome).toBe("found");
    expect(mockExecFile.mock.calls.map(([, args]) => args[4])).toEqual([
      "Claude Code-credentials-ee16a9f4",
      "Claude Code-credentials",
    ]);
  });

  it("scoped 항목을 읽지 못하면 legacy 항목이 없다고 오인하지 않는다", async () => {
    resolveExecFileWith(createSecurityFailure(36), "");

    await expect(readClaudeKeychainCredentials("/Users/tester/.claude"))
      .resolves.toEqual({ outcome: "unreadable" });
    expect(mockExecFile).toHaveBeenCalledTimes(1);
  });

  it("조회에 성공하면 자격증명 JSON을 돌려준다", async () => {
    resolveExecFileWith(null, `${createCredentialsJson("keychain-token")}\n`);

    const result = await readClaudeKeychainCredentials();

    expect(result.outcome).toBe("found");
    expect(readClaudeAccessToken(result.outcome === "found" ? result.credentials : "")).toBe("keychain-token");
  });

  it("항목이 없는 것은 로그인하지 않은 상태로 본다", async () => {
    resolveExecFileWith(createSecurityFailure(44), "");

    await expect(readClaudeKeychainCredentials()).resolves.toEqual({ outcome: "absent" });
  });

  it("접근이 거부되면 로그인하지 않은 상태와 구분한다", async () => {
    resolveExecFileWith(createSecurityFailure(36), "");

    await expect(readClaudeKeychainCredentials()).resolves.toEqual({ outcome: "unreadable" });
  });

  it("시간 초과처럼 사유를 모르는 실패도 읽지 못한 것으로 다룬다", async () => {
    resolveExecFileWith(Object.assign(new Error("timed out"), { killed: true }), "");

    await expect(readClaudeKeychainCredentials()).resolves.toEqual({ outcome: "unreadable" });
  });

  it("사용자 승인 프롬프트에서 멈추지 않도록 시간 제한을 건다", async () => {
    resolveExecFileWith(null, "");

    await readClaudeKeychainCredentials();

    const [, , options] = mockExecFile.mock.calls[0];
    expect(options.timeout).toBeGreaterThan(0);
  });
});
