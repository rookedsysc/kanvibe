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

/** `/Users/tester/.claude`를 sha256으로 줄인 Claude Code 2.1+ 서비스 이름 */
const TESTER_SCOPED_SERVICE = "Claude Code-credentials-ee16a9f4";
const LEGACY_SERVICE = "Claude Code-credentials";

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

/** Claude Code 2.1.x가 접미사 없는 항목에 MCP 서버 토큰만 남겨 둔 모양 */
const MCP_ONLY_CREDENTIALS = JSON.stringify({
  mcpOAuth: { "slack|38801a7d": { serverName: "slack", accessToken: "" } },
});

interface KeychainItemStub {
  credentials?: string;
  exitCode?: number;
}

/** 서비스 이름별로 응답을 정하고, 목록에 없는 이름은 "항목 없음"으로 답한다 */
function stubKeychainItems(items: Record<string, KeychainItemStub>, dumpOutput?: string): void {
  mockExecFile.mockImplementation((_command, args: string[], _options, callback) => {
    if (args[0] === "dump-keychain") {
      callback(dumpOutput ? null : createSecurityFailure(1), dumpOutput ?? "", "");
      return;
    }

    const item = items[args[args.indexOf("-s") + 1]];
    if (!item) {
      callback(createSecurityFailure(44), "", "");
      return;
    }

    callback(item.exitCode ? createSecurityFailure(item.exitCode) : null, item.credentials ?? "", "");
  });
}

function getRequestedServices(): string[] {
  return mockExecFile.mock.calls
    .map((call) => call[1] as string[])
    .filter((args) => args[0] === "find-generic-password")
    .map((args) => args[args.indexOf("-s") + 1]);
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

  it("MCP 서버 토큰만 담긴 자격증명은 로그인 토큰으로 보지 않는다", () => {
    expect(readClaudeAccessToken(MCP_ONLY_CREDENTIALS)).toBeNull();
  });
});

describe("readClaudeKeychainCredentials", () => {
  let warnSpy: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    stubPlatform("darwin");
    warnSpy = vi.spyOn(console, "warn").mockImplementation(() => {});
  });

  afterEach(() => {
    stubPlatform(originalPlatform);
    warnSpy.mockRestore();
    vi.resetAllMocks();
  });

  it("macOS가 아니면 Keychain을 건드리지 않는다", async () => {
    stubPlatform("linux");

    expect(await readClaudeKeychainCredentials(["/Users/tester/.claude"])).toEqual({ outcome: "absent" });
    expect(mockExecFile).not.toHaveBeenCalled();
  });

  it("PATH에 기대지 않도록 security를 절대경로로 실행한다", async () => {
    resolveExecFileWith(null, createCredentialsJson("keychain-token"));

    await readClaudeKeychainCredentials(["/Users/tester/.claude"]);

    expect(mockExecFile.mock.calls[0][0]).toBe("/usr/bin/security");
  });

  it("config dir의 scoped 서비스를 현재 사용자 계정으로 먼저 조회한다", async () => {
    resolveExecFileWith(null, createCredentialsJson("scoped-token"));

    const result = await readClaudeKeychainCredentials(["/Users/tester/.claude"]);

    expect(result).toEqual({ outcome: "found", credentials: createCredentialsJson("scoped-token") });
    expect(mockExecFile.mock.calls[0][1]).toEqual([
      "find-generic-password",
      "-a",
      expect.any(String),
      "-s",
      TESTER_SCOPED_SERVICE,
      "-w",
    ]);
    expect(mockExecFile).toHaveBeenCalledTimes(1);
  });

  it("scoped 항목이 없으면 legacy 서비스의 자격증명으로 폴백한다", async () => {
    stubKeychainItems({ [LEGACY_SERVICE]: { credentials: createCredentialsJson("legacy-token") } });

    const result = await readClaudeKeychainCredentials(["/Users/tester/.claude"]);

    expect(result).toEqual({ outcome: "found", credentials: createCredentialsJson("legacy-token") });
    expect(getRequestedServices()).toContain(LEGACY_SERVICE);
  });

  it("로그인 토큰이 없는 항목이 뒤 후보를 가리지 않는다", async () => {
    stubKeychainItems({
      [TESTER_SCOPED_SERVICE]: { credentials: MCP_ONLY_CREDENTIALS },
      [LEGACY_SERVICE]: { credentials: createCredentialsJson("legacy-token") },
    });

    const result = await readClaudeKeychainCredentials(["/Users/tester/.claude"]);

    expect(result).toEqual({ outcome: "found", credentials: createCredentialsJson("legacy-token") });
  });

  it("어느 항목에도 로그인 토큰이 없으면 로그인하지 않은 상태로 본다", async () => {
    stubKeychainItems({
      [TESTER_SCOPED_SERVICE]: { credentials: MCP_ONLY_CREDENTIALS },
      [LEGACY_SERVICE]: { credentials: MCP_ONLY_CREDENTIALS },
    });

    await expect(readClaudeKeychainCredentials(["/Users/tester/.claude"]))
      .resolves.toEqual({ outcome: "absent" });
  });

  it("현재 사용자 계정으로 못 찾으면 같은 서비스를 계정 없이 한 번 더 조회한다", async () => {
    mockExecFile.mockImplementation((_command, args: string[], _options, callback) => {
      if (args.includes("-a")) {
        callback(createSecurityFailure(44), "", "");
        return;
      }

      callback(null, createCredentialsJson("other-account-token"), "");
    });

    const result = await readClaudeKeychainCredentials(["/Users/tester/.claude"]);

    expect(result).toEqual({
      outcome: "found",
      credentials: createCredentialsJson("other-account-token"),
    });
    expect(mockExecFile.mock.calls[1][1]).toEqual([
      "find-generic-password",
      "-s",
      TESTER_SCOPED_SERVICE,
      "-w",
    ]);
  });

  it("config dir 후보가 여럿이면 각 후보의 scoped 서비스를 모두 조회한다", async () => {
    stubKeychainItems({});

    await readClaudeKeychainCredentials(["/Users/tester/.claude", "/Users/tester/.claude-work"]);

    expect(getRequestedServices()).toContain(TESTER_SCOPED_SERVICE);
    expect(getRequestedServices()).toContain("Claude Code-credentials-c4394a73");
  });

  it("이름을 추측하지 못하면 실제 Keychain 항목 목록에서 찾아낸다", async () => {
    const storedService = "Claude Code-credentials-deadbeef";
    stubKeychainItems(
      { [storedService]: { credentials: createCredentialsJson("stored-token") } },
      `    "acct"<blob>="tester"\n    "svce"<blob>="${storedService}"\n`,
    );

    const result = await readClaudeKeychainCredentials(["/Users/tester/.claude"]);

    expect(result).toEqual({ outcome: "found", credentials: createCredentialsJson("stored-token") });
    expect(getRequestedServices()).toContain(storedService);
  });

  it("목록 조회는 비밀값을 복호화하지 않고, 추측이 통하면 아예 실행하지 않는다", async () => {
    stubKeychainItems({ [TESTER_SCOPED_SERVICE]: { credentials: createCredentialsJson("scoped-token") } });

    await readClaudeKeychainCredentials(["/Users/tester/.claude"]);
    expect(mockExecFile.mock.calls.some(([, args]) => args[0] === "dump-keychain")).toBe(false);

    mockExecFile.mockReset();
    stubKeychainItems({}, `    "svce"<blob>="Claude Code-credentials-deadbeef"\n`);
    await readClaudeKeychainCredentials(["/Users/tester/.claude"]);

    const dumpArgs = mockExecFile.mock.calls.find(([, args]) => args[0] === "dump-keychain")?.[1];
    expect(dumpArgs).toEqual(["dump-keychain"]);
  });

  it("목록에서 다른 앱의 항목은 조회하지 않는다", async () => {
    stubKeychainItems(
      {},
      `    "svce"<blob>="com.apple.Safari"\n    "svce"<blob>="GitHub - api.github.com"\n`,
    );

    await readClaudeKeychainCredentials(["/Users/tester/.claude"]);

    expect(getRequestedServices()).toEqual([
      TESTER_SCOPED_SERVICE,
      TESTER_SCOPED_SERVICE,
      LEGACY_SERVICE,
      LEGACY_SERVICE,
    ]);
  });

  it("항목이 없는 것은 로그인하지 않은 상태로 본다", async () => {
    resolveExecFileWith(createSecurityFailure(44), "");

    await expect(readClaudeKeychainCredentials(["/Users/tester/.claude"]))
      .resolves.toEqual({ outcome: "absent" });
  });

  it("접근이 거부되면 로그인하지 않은 상태와 구분한다", async () => {
    resolveExecFileWith(createSecurityFailure(36), "");

    await expect(readClaudeKeychainCredentials(["/Users/tester/.claude"]))
      .resolves.toEqual({ outcome: "unreadable" });
  });

  it("읽지 못한 항목이 있어도 나머지 후보에서 토큰을 찾으면 그 값을 쓴다", async () => {
    stubKeychainItems({
      [TESTER_SCOPED_SERVICE]: { exitCode: 36 },
      [LEGACY_SERVICE]: { credentials: createCredentialsJson("legacy-token") },
    });

    await expect(readClaudeKeychainCredentials(["/Users/tester/.claude"]))
      .resolves.toEqual({ outcome: "found", credentials: createCredentialsJson("legacy-token") });
  });

  it("읽지 못한 항목은 계정 없이 다시 묻지 않는다", async () => {
    stubKeychainItems({ [TESTER_SCOPED_SERVICE]: { exitCode: 36 } });

    await readClaudeKeychainCredentials(["/Users/tester/.claude"]);

    expect(getRequestedServices().filter((service) => service === TESTER_SCOPED_SERVICE))
      .toEqual([TESTER_SCOPED_SERVICE]);
  });

  it("찾지 못하면 시도한 서비스 이름과 결과만 남기고 자격증명 값은 남기지 않는다", async () => {
    stubKeychainItems({ [LEGACY_SERVICE]: { credentials: MCP_ONLY_CREDENTIALS } });

    await readClaudeKeychainCredentials(["/Users/tester/.claude"]);

    const [message] = warnSpy.mock.calls[0] as [string];
    expect(message).toContain(`${TESTER_SCOPED_SERVICE}=absent`);
    expect(message).toContain(`${LEGACY_SERVICE}=found`);
    expect(message).not.toContain("mcpOAuth");
  });

  it("시간 초과처럼 사유를 모르는 실패도 읽지 못한 것으로 다룬다", async () => {
    resolveExecFileWith(Object.assign(new Error("timed out"), { killed: true }), "");

    await expect(readClaudeKeychainCredentials(["/Users/tester/.claude"]))
      .resolves.toEqual({ outcome: "unreadable" });
  });

  it("사용자 승인 프롬프트에서 멈추지 않도록 시간 제한을 건다", async () => {
    resolveExecFileWith(null, "");

    await readClaudeKeychainCredentials(["/Users/tester/.claude"]);

    const [, , options] = mockExecFile.mock.calls[0];
    expect(options.timeout).toBeGreaterThan(0);
  });
});
