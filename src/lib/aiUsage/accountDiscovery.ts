import { readFile, readdir } from "fs/promises";
import { homedir } from "os";
import path from "path";
import type { AiAccountRegistration } from "@/lib/aiUsage/accountRegistry";
import {
  readClaudeAccessToken,
  readClaudeKeychainCredentials,
} from "@/lib/aiUsage/claudeCredentials";
import {
  AI_PROVIDER_CONFIG_DIR_SPECS,
  toAccountConfigDir,
  toAccountNameFromRoot,
  toDefaultAccountRoot,
  type AiProviderConfigDirSpec,
} from "@/lib/aiUsage/providerConfigDir";
import type { AiUsageAccount, AiUsageProvider } from "@/lib/aiUsage/types";

interface ProviderAccountIdentity {
  accountId: string | null;
  label: string | null;
}

/**
 * 조회를 시도할 계정 루트 후보를 순서대로 모은다.
 *
 * 환경변수로 지정한 루트를 가장 신뢰하고, 그다음이 홈의 기본 루트다.
 * 계정을 여러 개 쓰는 사람은 `~/.claude-work`처럼 접두사가 같은 형제 루트를 만들어
 * 환경변수로 갈아 끼우는 관행을 쓰므로 그 형제들도 후보로 넣고, KanVibe가 만든 계정도 더한다.
 */
async function collectAccountRootCandidates(
  spec: AiProviderConfigDirSpec,
  registrations: AiAccountRegistration[],
): Promise<string[]> {
  const homeDirectory = homedir();
  const candidates: string[] = [];

  const configuredRoot = process.env[spec.homeEnvVar]?.trim();
  if (configuredRoot) {
    candidates.push(path.resolve(configuredRoot));
  }

  candidates.push(toDefaultAccountRoot(spec, homeDirectory));

  try {
    const homeEntries = await readdir(homeDirectory, { withFileTypes: true });
    for (const entry of homeEntries) {
      if (entry.isDirectory() && entry.name.startsWith(spec.siblingRootPrefix)) {
        candidates.push(path.join(homeDirectory, entry.name));
      }
    }
  } catch {
    // 홈을 못 읽으면 기본 후보만으로 진행한다
  }

  candidates.push(...registrations.map((registration) => registration.accountRoot));

  return [...new Set(candidates)];
}

async function readJsonFile(filePath: string): Promise<Record<string, unknown> | null> {
  try {
    const parsed = JSON.parse(await readFile(filePath, "utf-8")) as unknown;
    return parsed && typeof parsed === "object" && !Array.isArray(parsed)
      ? (parsed as Record<string, unknown>)
      : null;
  } catch {
    return null;
  }
}

/**
 * 계정 정보가 담긴 `oauthAccount`를 찾는다.
 *
 * config dir 안에도 같은 이름의 파일이 생기지만 계정 정보 없이 만들어지는 판이 있어,
 * 파일이 있다는 사실만으로는 계정을 찾았다고 볼 수 없다.
 * 홈 루트의 파일은 기본 계정의 것이므로 다른 계정에는 빌려주지 않는다.
 * 빌려주면 계정마다 같은 accountUuid를 달게 되어 중복 제거가 계정 하나를 삼킨다.
 */
async function readClaudeOauthAccount(
  configDir: string,
): Promise<Record<string, unknown> | undefined> {
  const colocatedConfig = await readJsonFile(path.join(configDir, ".claude.json"));
  const colocatedAccount = colocatedConfig?.oauthAccount as Record<string, unknown> | undefined;
  const isDefaultAccount = configDir === toDefaultAccountRoot(AI_PROVIDER_CONFIG_DIR_SPECS.claude);
  if (colocatedAccount || !isDefaultAccount) {
    return colocatedAccount;
  }

  const homeConfig = await readJsonFile(path.join(homedir(), ".claude.json"));
  return homeConfig?.oauthAccount as Record<string, unknown> | undefined;
}

/** Claude는 사용량 응답에 계정 정보를 담지 않아서 `.claude.json`이 유일한 식별·라벨 출처다 */
async function readClaudeAccountIdentity(configDir: string): Promise<ProviderAccountIdentity> {
  const oauthAccount = await readClaudeOauthAccount(configDir);

  const accountUuid = oauthAccount?.accountUuid;
  const emailAddress = oauthAccount?.emailAddress;
  const displayName = oauthAccount?.displayName;

  return {
    accountId: typeof accountUuid === "string" && accountUuid.trim() ? accountUuid : null,
    label: firstNonEmptyString([emailAddress, displayName]),
  };
}

async function readCodexAccountIdentity(configDir: string): Promise<ProviderAccountIdentity> {
  const auth = await readJsonFile(path.join(configDir, "auth.json"));
  const tokens = auth?.tokens as Record<string, unknown> | undefined;

  return {
    accountId: firstNonEmptyString([tokens?.account_id]),
    label: readEmailFromIdToken(tokens?.id_token),
  };
}

/**
 * Gemini CLI는 계정 이메일을 남기지 않는 판이 있고, 자격증명을 OS 키체인에 넣는 판도 있다.
 * 어느 쪽이든 계정을 가를 안정적인 로컬 값이 경로뿐이라 계정 이름을 라벨로 쓴다.
 */
async function readGeminiAccountIdentity(): Promise<ProviderAccountIdentity> {
  return { accountId: null, label: null };
}

const ACCOUNT_IDENTITY_READERS: Record<
  AiUsageProvider,
  (configDir: string) => Promise<ProviderAccountIdentity>
> = {
  claude: readClaudeAccountIdentity,
  codex: readCodexAccountIdentity,
  gemini: readGeminiAccountIdentity,
};

/** provider마다 자격증명 파일에서 로그인을 증명하는 자리가 다르다 */
const STORED_ACCESS_TOKEN_READERS: Record<
  AiUsageProvider,
  (credentials: Record<string, unknown>) => string | null
> = {
  claude: (credentials) => {
    const oauth = credentials.claudeAiOauth as Record<string, unknown> | undefined;
    return firstNonEmptyString([oauth?.accessToken]);
  },
  codex: (credentials) => {
    const tokens = credentials.tokens as Record<string, unknown> | undefined;
    return firstNonEmptyString([tokens?.access_token]);
  },
  gemini: (credentials) => firstNonEmptyString([credentials.access_token]),
};

/** 이 디렉터리에 로그인을 증명하는 자격증명이 들어 있는지 */
export async function hasStoredCredentials(
  spec: AiProviderConfigDirSpec,
  configDir: string,
): Promise<boolean> {
  const credentials = await readJsonFile(path.join(configDir, spec.credentialsFileName));
  return Boolean(credentials && STORED_ACCESS_TOKEN_READERS[spec.provider](credentials));
}

/** id_token은 서명 검증 없이 표시용 이메일만 꺼낸다. 로컬 파일이고 인증에 쓰지 않는다 */
function readEmailFromIdToken(idToken: unknown): string | null {
  if (typeof idToken !== "string") {
    return null;
  }

  const payloadSegment = idToken.split(".")[1];
  if (!payloadSegment) {
    return null;
  }

  try {
    const payload = JSON.parse(Buffer.from(payloadSegment, "base64url").toString("utf-8")) as {
      email?: unknown;
    };
    return firstNonEmptyString([payload.email]);
  } catch {
    return null;
  }
}

function firstNonEmptyString(values: unknown[]): string | null {
  for (const value of values) {
    if (typeof value === "string" && value.trim()) {
      return value.trim();
    }
  }
  return null;
}

/** 같은 계정이 여러 경로에서 발견되면 가장 신뢰도 높은 첫 후보만 남긴다 */
function deduplicateAccounts(accounts: AiUsageAccount[]): AiUsageAccount[] {
  const seenAccountIds = new Set<string>();
  return accounts.filter((account) => {
    if (seenAccountIds.has(account.accountId)) {
      return false;
    }
    seenAccountIds.add(account.accountId);
    return true;
  });
}

/**
 * macOS의 Claude Code는 자격증명을 Keychain에 두고 `.credentials.json`은 폴백으로만 쓴다.
 * 그래서 config dir에 파일이 하나도 없어도 로그인되어 있을 수 있다.
 *
 * Keychain을 읽지 못한 경우에도 계정 자리를 만든다. 조회 단계가 그 사유를 화면에 알려야 하는데,
 * 여기서 없는 계정으로 처리하면 "로그인되어 있지 않습니다"로 뭉개진다.
 *
 * 항목은 config dir마다 다른 이름으로 저장되므로 후보를 모두 넘긴다.
 * 어느 후보에서 나왔는지는 Keychain이 알려주지 않아, 계정 정보는 홈의 기본 디렉터리에서 읽는다.
 */
async function discoverClaudeKeychainAccount(configDirs: string[]): Promise<AiUsageAccount | null> {
  const configDir = toDefaultAccountRoot(AI_PROVIDER_CONFIG_DIR_SPECS.claude);
  const keychainResult = await readClaudeKeychainCredentials(configDirs);
  const hasAccount = keychainResult.outcome === "unreadable"
    || (keychainResult.outcome === "found" && Boolean(readClaudeAccessToken(keychainResult.credentials)));
  if (!hasAccount) {
    return null;
  }

  const identity = await readClaudeAccountIdentity(configDir);
  return {
    provider: "claude",
    accountId: identity.accountId ?? configDir,
    label: identity.label ?? AI_PROVIDER_CONFIG_DIR_SPECS.claude.defaultLabel,
    configDir,
    accountRoot: configDir,
  };
}

function toAccountLabel(
  spec: AiProviderConfigDirSpec,
  identity: ProviderAccountIdentity,
  accountRoot: string,
): string {
  return identity.label ?? toAccountNameFromRoot(spec, accountRoot) ?? spec.defaultLabel;
}

/**
 * provider 하나의 계정을 모두 찾는다.
 *
 * KanVibe에서 만든 계정은 로그아웃돼 있어도 목록에 남긴다. 자리를 지워 버리면 화면에서
 * 다시 로그인할 곳이 사라져, 사용자가 다시 터미널로 나가야 하는 지금 문제로 되돌아간다.
 */
export async function discoverProviderAccounts(
  provider: AiUsageProvider,
  registrations: AiAccountRegistration[] = [],
): Promise<AiUsageAccount[]> {
  const spec = AI_PROVIDER_CONFIG_DIR_SPECS[provider];
  const providerRegistrations = registrations.filter(
    (registration) => registration.provider === provider,
  );
  const registeredRoots = new Set(
    providerRegistrations.map((registration) => registration.accountRoot),
  );
  const candidateRoots = await collectAccountRootCandidates(spec, providerRegistrations);
  const accounts: AiUsageAccount[] = [];

  for (const accountRoot of candidateRoots) {
    const configDir = toAccountConfigDir(spec, accountRoot);
    const isRegistered = registeredRoots.has(accountRoot);
    if (!isRegistered && !(await hasStoredCredentials(spec, configDir))) {
      continue;
    }

    const identity = await ACCOUNT_IDENTITY_READERS[provider](configDir);
    accounts.push({
      provider,
      accountId: identity.accountId ?? accountRoot,
      label: toAccountLabel(spec, identity, accountRoot),
      configDir,
      accountRoot,
    });
  }

  // 기본 계정을 파일로 찾았으면 Keychain 승인 프롬프트를 띄울 이유가 없다.
  // 다른 계정을 등록해 뒀다는 사실은 기본 계정이 Keychain에 있는지와 무관하므로 개수로 판단하지 않는다
  const defaultRoot = toDefaultAccountRoot(spec);
  const hasDefaultAccount = accounts.some((account) => account.accountRoot === defaultRoot);
  if (provider === "claude" && !hasDefaultAccount) {
    const keychainAccount = await discoverClaudeKeychainAccount(candidateRoots);
    if (keychainAccount) {
      accounts.push(keychainAccount);
    }
  }

  return deduplicateAccounts(accounts);
}
