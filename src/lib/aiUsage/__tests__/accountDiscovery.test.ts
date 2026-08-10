import { mkdir, mkdtemp, rm, writeFile } from "fs/promises";
import { tmpdir } from "os";
import path from "path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { discoverClaudeAccounts, discoverCodexAccounts } from "@/lib/aiUsage/accountDiscovery";

/** os.homedir()는 POSIX에서 $HOME을 읽으므로 모듈을 모킹하지 않고 환경변수로 홈을 바꾼다 */
let fakeHome: string;

async function writeClaudeConfigDir(
  directoryName: string,
  account: { accountUuid: string; emailAddress?: string; displayName?: string },
  options: { colocatedConfig?: boolean } = {},
) {
  const configDir = path.join(fakeHome, directoryName);
  await mkdir(configDir, { recursive: true });
  await writeFile(
    path.join(configDir, ".credentials.json"),
    JSON.stringify({ claudeAiOauth: { accessToken: "token", refreshToken: "refresh" } }),
    "utf-8",
  );

  const configPath = options.colocatedConfig
    ? path.join(configDir, ".claude.json")
    : path.join(fakeHome, ".claude.json");
  await writeFile(configPath, JSON.stringify({ oauthAccount: account }), "utf-8");
  return configDir;
}

describe("discoverClaudeAccounts", () => {
  beforeEach(async () => {
    fakeHome = await mkdtemp(path.join(tmpdir(), "kanvibe-home-"));
    vi.stubEnv("HOME", fakeHome);
  });

  afterEach(async () => {
    await rm(fakeHome, { recursive: true, force: true });
    vi.unstubAllEnvs();
    vi.resetAllMocks();
  });

  it("기본 홈 디렉터리의 계정을 찾고 이메일을 라벨로 쓴다", async () => {
    vi.stubEnv("CLAUDE_CONFIG_DIR", "");
    await writeClaudeConfigDir(".claude", {
      accountUuid: "uuid-default",
      emailAddress: "me@example.com",
      displayName: "Me",
    });

    const accounts = await discoverClaudeAccounts();

    expect(accounts).toHaveLength(1);
    expect(accounts[0].provider).toBe("claude");
    expect(accounts[0].accountId).toBe("uuid-default");
    expect(accounts[0].label).toBe("me@example.com");
    expect(accounts[0].configDir).toBe(path.join(fakeHome, ".claude"));
  });

  it("이메일이 없으면 표시 이름으로, 둘 다 없으면 기본 라벨로 내려간다", async () => {
    vi.stubEnv("CLAUDE_CONFIG_DIR", "");
    await writeClaudeConfigDir(".claude", { accountUuid: "uuid-name-only", displayName: "Work Seat" });

    expect((await discoverClaudeAccounts())[0].label).toBe("Work Seat");

    await writeFile(path.join(fakeHome, ".claude.json"), JSON.stringify({}), "utf-8");
    const withoutIdentity = await discoverClaudeAccounts();
    expect(withoutIdentity[0].label).toBe("Claude");
  });

  it("형제 디렉터리에 등록된 두 번째 계정도 찾는다", async () => {
    vi.stubEnv("CLAUDE_CONFIG_DIR", "");
    await writeClaudeConfigDir(".claude", { accountUuid: "uuid-personal", emailAddress: "me@example.com" });
    await writeClaudeConfigDir(
      ".claude-work",
      { accountUuid: "uuid-work", emailAddress: "work@example.com" },
      { colocatedConfig: true },
    );

    const labels = (await discoverClaudeAccounts()).map((account) => account.label).sort();

    expect(labels).toEqual(["me@example.com", "work@example.com"]);
  });

  it("같은 계정이 두 경로에서 발견돼도 한 번만 돌려준다", async () => {
    await writeClaudeConfigDir(".claude", { accountUuid: "uuid-same", emailAddress: "me@example.com" });
    await writeClaudeConfigDir(
      ".claude-mirror",
      { accountUuid: "uuid-same", emailAddress: "me@example.com" },
      { colocatedConfig: true },
    );
    vi.stubEnv("CLAUDE_CONFIG_DIR", path.join(fakeHome, ".claude"));

    const accounts = await discoverClaudeAccounts();

    expect(accounts).toHaveLength(1);
    expect(accounts[0].configDir).toBe(path.join(fakeHome, ".claude"));
  });

  it("자격증명이 없는 디렉터리는 계정으로 세지 않는다", async () => {
    vi.stubEnv("CLAUDE_CONFIG_DIR", "");
    await mkdir(path.join(fakeHome, ".claude-empty"), { recursive: true });

    expect(await discoverClaudeAccounts()).toEqual([]);
  });
});

describe("discoverCodexAccounts", () => {
  beforeEach(async () => {
    fakeHome = await mkdtemp(path.join(tmpdir(), "kanvibe-home-"));
    vi.stubEnv("HOME", fakeHome);
    vi.stubEnv("CODEX_HOME", "");
  });

  afterEach(async () => {
    await rm(fakeHome, { recursive: true, force: true });
    vi.unstubAllEnvs();
    vi.resetAllMocks();
  });

  it("auth.json의 account_id로 계정을 식별하고 id_token의 이메일을 라벨로 쓴다", async () => {
    const idTokenPayload = Buffer.from(JSON.stringify({ email: "codex@example.com" })).toString("base64url");
    const configDir = path.join(fakeHome, ".codex");
    await mkdir(configDir, { recursive: true });
    await writeFile(
      path.join(configDir, "auth.json"),
      JSON.stringify({
        tokens: { access_token: "token", account_id: "acct-1", id_token: `header.${idTokenPayload}.sig` },
      }),
      "utf-8",
    );

    const accounts = await discoverCodexAccounts();

    expect(accounts).toHaveLength(1);
    expect(accounts[0].accountId).toBe("acct-1");
    expect(accounts[0].label).toBe("codex@example.com");
  });

  it("액세스 토큰이 없는 auth.json은 계정으로 세지 않는다", async () => {
    const configDir = path.join(fakeHome, ".codex");
    await mkdir(configDir, { recursive: true });
    await writeFile(path.join(configDir, "auth.json"), JSON.stringify({ tokens: null }), "utf-8");

    expect(await discoverCodexAccounts()).toEqual([]);
  });
});
