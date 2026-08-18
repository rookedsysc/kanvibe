import { mkdtemp, rm, stat, writeFile, mkdir } from "fs/promises";
import { tmpdir } from "os";
import path from "path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  addAiAccount,
  getAiAccountRegistrations,
  listAiAccounts,
  removeAiAccount,
} from "@/desktop/main/services/aiAccountService";

const {
  mockGetAppSetting,
  mockSetAppSetting,
  mockReadProviderAuthStatus,
  mockLogoutThroughCli,
} = vi.hoisted(() => ({
  mockGetAppSetting: vi.fn(),
  mockSetAppSetting: vi.fn(),
  mockReadProviderAuthStatus: vi.fn(),
  mockLogoutThroughCli: vi.fn(),
}));

vi.mock("@/desktop/main/services/appSettingsService", () => ({
  getAppSetting: mockGetAppSetting,
  setAppSetting: mockSetAppSetting,
}));

vi.mock("@/lib/aiUsage/providerCli", () => ({
  readProviderAuthStatus: mockReadProviderAuthStatus,
  logoutThroughCli: mockLogoutThroughCli,
}));

/** os.homedir()는 POSIX에서 $HOME을 읽으므로 모듈을 모킹하지 않고 환경변수로 홈을 바꾼다 */
let fakeHome: string;

/** 앱 설정 저장소를 흉내 낸다. 저장한 값을 다음 조회가 그대로 보게 해야 등록 흐름을 검증할 수 있다 */
function useInMemoryAppSettings(): void {
  const storedSettings = new Map<string, string>();
  mockGetAppSetting.mockImplementation(async (key: string) => storedSettings.get(key) ?? null);
  mockSetAppSetting.mockImplementation(async (key: string, value: string) => {
    storedSettings.set(key, value);
  });
}

async function directoryExists(targetPath: string): Promise<boolean> {
  try {
    return (await stat(targetPath)).isDirectory();
  } catch {
    return false;
  }
}

beforeEach(async () => {
  fakeHome = await mkdtemp(path.join(tmpdir(), "kanvibe-home-"));
  vi.stubEnv("HOME", fakeHome);
  vi.stubEnv("CLAUDE_CONFIG_DIR", "");
  vi.stubEnv("CODEX_HOME", "");
  vi.stubEnv("GEMINI_CLI_HOME", "");
  useInMemoryAppSettings();
  mockReadProviderAuthStatus.mockResolvedValue(null);
  mockLogoutThroughCli.mockResolvedValue(true);
});

afterEach(async () => {
  await rm(fakeHome, { recursive: true, force: true });
  vi.unstubAllEnvs();
  vi.resetAllMocks();
});

describe("addAiAccount", () => {
  it("계정 자리를 만들고 등록한다", async () => {
    const result = await addAiAccount("claude", "work");

    expect(result.outcome).toBe("ok");
    expect(result.accountRoot).toBe(path.join(fakeHome, ".claude-work"));
    expect(await directoryExists(path.join(fakeHome, ".claude-work"))).toBe(true);
    expect(await getAiAccountRegistrations()).toEqual([
      { provider: "claude", accountRoot: path.join(fakeHome, ".claude-work"), accountName: "work" },
    ]);
  });

  it("Gemini는 루트 아래 .gemini까지 만들어 CLI가 그 자리를 쓰게 한다", async () => {
    const result = await addAiAccount("gemini", "work");

    expect(result.accountRoot).toBe(path.join(fakeHome, ".gemini-work"));
    expect(await directoryExists(path.join(fakeHome, ".gemini-work", ".gemini"))).toBe(true);
  });

  it("홈 밖으로 나갈 수 있는 이름은 거절하고 디렉터리를 만들지 않기까지 한다", async () => {
    expect((await addAiAccount("claude", "../escape")).outcome).toBe("invalid-name");
    expect((await addAiAccount("claude", "with space")).outcome).toBe("invalid-name");
    expect(await getAiAccountRegistrations()).toEqual([]);
    expect(mockSetAppSetting).not.toHaveBeenCalled();
  });

  it("같은 이름을 다시 등록하려 하면 알린다", async () => {
    await addAiAccount("claude", "work");

    expect((await addAiAccount("claude", "work")).outcome).toBe("already-exists");
  });
});

describe("listAiAccounts", () => {
  it("등록해 둔 계정은 로그아웃돼 있어도 목록에 남아 로그인할 자리를 준다", async () => {
    await addAiAccount("claude", "work");

    const accounts = await listAiAccounts();
    const workAccount = accounts.find(
      (account) => account.accountRoot === path.join(fakeHome, ".claude-work"),
    );

    expect(workAccount?.isLoggedIn).toBe(false);
    expect(workAccount?.isRemovable).toBe(true);
    expect(workAccount?.accountName).toBe("work");
  });

  it("CLI가 알려준 계정 이름과 구독 등급을 그대로 쓴다", async () => {
    await addAiAccount("claude", "work");
    mockReadProviderAuthStatus.mockResolvedValue({
      isLoggedIn: true,
      label: "work@example.com",
      planName: "max",
    });

    const workAccount = (await listAiAccounts()).find(
      (account) => account.accountRoot === path.join(fakeHome, ".claude-work"),
    );

    expect(workAccount?.label).toBe("work@example.com");
    expect(workAccount?.planName).toBe("max");
    expect(workAccount?.isLoggedIn).toBe(true);
  });

  it("CLI가 답하지 못하면 자격증명 파일로 로그인 여부를 판단한다", async () => {
    const configDir = path.join(fakeHome, ".codex");
    await mkdir(configDir, { recursive: true });
    await writeFile(
      path.join(configDir, "auth.json"),
      JSON.stringify({ tokens: { access_token: "token" } }),
      "utf-8",
    );

    const codexAccount = (await listAiAccounts()).find(
      (account) => account.provider === "codex",
    );

    expect(codexAccount?.isLoggedIn).toBe(true);
  });

  it("사용자가 직접 만든 디렉터리는 앱에서 지울 수 있는 계정으로 표시하지 않는다", async () => {
    const configDir = path.join(fakeHome, ".codex-manual");
    await mkdir(configDir, { recursive: true });
    await writeFile(
      path.join(configDir, "auth.json"),
      JSON.stringify({ tokens: { access_token: "token" } }),
      "utf-8",
    );

    const manualAccount = (await listAiAccounts()).find(
      (account) => account.accountRoot === configDir,
    );

    expect(manualAccount?.isRemovable).toBe(false);
  });
});

describe("removeAiAccount", () => {
  it("등록된 계정은 로그아웃한 뒤 자리까지 치운다", async () => {
    const { accountRoot } = await addAiAccount("claude", "work");

    const result = await removeAiAccount("claude", accountRoot!);

    expect(result.outcome).toBe("ok");
    expect(mockLogoutThroughCli).toHaveBeenCalledWith("claude", accountRoot);
    expect(await directoryExists(accountRoot!)).toBe(false);
    expect(await getAiAccountRegistrations()).toEqual([]);
  });

  it("KanVibe가 만들지 않은 자리는 로그아웃만 하고 남겨 둔다", async () => {
    const manualRoot = path.join(fakeHome, ".claude-manual");
    await mkdir(manualRoot, { recursive: true });

    const result = await removeAiAccount("claude", manualRoot);

    expect(result.outcome).toBe("ok");
    expect(await directoryExists(manualRoot)).toBe(true);
  });

  it("앱에서 로그아웃할 수 없는 provider는 사용자에게 사실대로 알린다", async () => {
    mockLogoutThroughCli.mockResolvedValue(false);

    const result = await removeAiAccount("gemini", path.join(fakeHome, ".gemini-manual"));

    expect(result.outcome).toBe("manual-logout-required");
  });
});
