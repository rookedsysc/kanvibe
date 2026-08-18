import { afterEach, describe, expect, it, vi } from "vitest";
import { AI_PROVIDER_CONFIG_DIR_SPECS } from "@/lib/aiUsage/providerConfigDir";
import {
  createProviderCliEnvironment,
  getProviderLoginCommand,
  readProviderAuthStatus,
  refreshCredentialsThroughCli,
} from "@/lib/aiUsage/providerCli";

const { mockExecFile } = vi.hoisted(() => ({ mockExecFile: vi.fn() }));

vi.mock("node:child_process", async (importOriginal) => {
  const actual = await importOriginal<typeof import("node:child_process")>();
  return {
    ...actual,
    default: { ...actual, execFile: (...args: unknown[]) => mockExecFile(...args) },
    execFile: (...args: unknown[]) => mockExecFile(...args),
  };
});

/** execFile 콜백은 (error, stdout, stderr) 순서로 불린다 */
function stubCliOutput(stdout: string, error: unknown = null): void {
  mockExecFile.mockImplementation((_command, _args, _options, callback) => {
    callback(error, stdout, "");
  });
}

afterEach(() => {
  vi.unstubAllEnvs();
  vi.resetAllMocks();
});

describe("createProviderCliEnvironment", () => {
  it("계정 위치를 알리는 변수 하나만 얹는다", () => {
    const environment = createProviderCliEnvironment(
      AI_PROVIDER_CONFIG_DIR_SPECS.gemini,
      "/home/tester/.gemini-work",
    );

    expect(environment.GEMINI_CLI_HOME).toBe("/home/tester/.gemini-work");
    expect(environment.CLAUDE_CONFIG_DIR).toBeUndefined();
    expect(environment.CODEX_HOME).toBeUndefined();
  });

  it("서버 런타임 값과 KanVibe 내부 값은 CLI로 넘기지 않는다", () => {
    vi.stubEnv("PORT", "3000");
    vi.stubEnv("HOST", "0.0.0.0");
    vi.stubEnv("NODE_ENV", "production");
    vi.stubEnv("KANVIBE_INTERNAL_TOKEN", "secret");

    const environment = createProviderCliEnvironment(
      AI_PROVIDER_CONFIG_DIR_SPECS.claude,
      "/home/tester/.claude",
    );

    expect(environment.PORT).toBeUndefined();
    expect(environment.HOST).toBeUndefined();
    expect(environment.NODE_ENV).toBeUndefined();
    expect(environment.KANVIBE_INTERNAL_TOKEN).toBeUndefined();
    expect(environment.CLAUDE_CONFIG_DIR).toBe("/home/tester/.claude");
  });
});

describe("getProviderLoginCommand", () => {
  it("Claude는 구독 로그인을 곧바로 고르게 해 선택 화면을 건너뛴다", () => {
    expect(getProviderLoginCommand("claude")).toEqual({
      command: "claude",
      args: ["auth", "login", "--claudeai"],
    });
  });

  it("Gemini는 로그인 전용 하위 명령이 없어 CLI를 그대로 띄운다", () => {
    expect(getProviderLoginCommand("gemini")).toEqual({ command: "gemini", args: [] });
  });
});

describe("readProviderAuthStatus", () => {
  it("Claude는 로그인 여부와 이메일과 구독 등급을 한 번에 알려준다", async () => {
    stubCliOutput(JSON.stringify({
      loggedIn: true,
      email: "me@example.com",
      subscriptionType: "max",
    }));

    expect(await readProviderAuthStatus("claude", "/home/tester/.claude")).toEqual({
      isLoggedIn: true,
      label: "me@example.com",
      planName: "max",
    });
  });

  it("Codex는 사람이 읽는 한 줄만 주므로 로그인 여부만 읽는다", async () => {
    stubCliOutput("Logged in using ChatGPT\n");

    expect(await readProviderAuthStatus("codex", "/home/tester/.codex")).toEqual({
      isLoggedIn: true,
      label: null,
      planName: null,
    });
  });

  it("Codex의 로그아웃 문구를 로그인으로 읽지 않는다", async () => {
    stubCliOutput("Not logged in\n");

    expect(await readProviderAuthStatus("codex", "/home/tester/.codex")).toEqual({
      isLoggedIn: false,
      label: null,
      planName: null,
    });
  });

  it("물을 수 없는 provider는 CLI를 부르지 않고 모른다고 답한다", async () => {
    expect(await readProviderAuthStatus("gemini", "/home/tester/.gemini-work")).toBeNull();
    expect(mockExecFile).not.toHaveBeenCalled();
  });

  it("CLI를 실행하지 못하면 로그아웃이 아니라 모른다로 둔다", async () => {
    stubCliOutput("", Object.assign(new Error("spawn ENOENT"), { code: "ENOENT" }));

    expect(await readProviderAuthStatus("claude", "/home/tester/.claude")).toBeNull();
  });

  it("해석할 수 없는 출력은 모른다로 둔다", async () => {
    stubCliOutput("not json at all");

    expect(await readProviderAuthStatus("claude", "/home/tester/.claude")).toBeNull();
  });
});

describe("refreshCredentialsThroughCli", () => {
  it("계정 루트를 CLI 환경에 실어 상태를 물으면서 갱신을 맡긴다", async () => {
    stubCliOutput(JSON.stringify({ loggedIn: true }));

    expect(await refreshCredentialsThroughCli("claude", "/home/tester/.claude-work")).toBe(true);
    const [command, args, options] = mockExecFile.mock.calls[0];
    expect(command).toBe("claude");
    expect(args).toEqual(["auth", "status", "--json"]);
    expect(options.env.CLAUDE_CONFIG_DIR).toBe("/home/tester/.claude-work");
  });

  it("CLI가 로그인 상태를 확인해 주지 못하면 다시 읽을 가치가 없다고 답한다", async () => {
    stubCliOutput(JSON.stringify({ loggedIn: false }));

    expect(await refreshCredentialsThroughCli("claude", "/home/tester/.claude")).toBe(false);
  });
});
