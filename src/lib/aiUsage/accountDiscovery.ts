import { readFile, readdir } from "fs/promises";
import { homedir } from "os";
import path from "path";
import {
  readClaudeAccessToken,
  readClaudeKeychainCredentials,
} from "@/lib/aiUsage/claudeCredentials";
import type { AiUsageAccount, AiUsageProvider } from "@/lib/aiUsage/types";

interface ProviderDiscoverySpec {
  provider: AiUsageProvider;
  /** 계정을 분리해 쓰는 사람들이 지정하는 config dir 환경변수 */
  configDirEnvVar: string;
  /** 홈 아래 기본 디렉터리이자 형제 디렉터리 탐색의 접두사 */
  defaultDirectoryName: string;
  credentialsFileName: string;
  defaultLabel: string;
}

const CLAUDE_DISCOVERY: ProviderDiscoverySpec = {
  provider: "claude",
  configDirEnvVar: "CLAUDE_CONFIG_DIR",
  defaultDirectoryName: ".claude",
  credentialsFileName: ".credentials.json",
  defaultLabel: "Claude",
};

const CODEX_DISCOVERY: ProviderDiscoverySpec = {
  provider: "codex",
  configDirEnvVar: "CODEX_HOME",
  defaultDirectoryName: ".codex",
  credentialsFileName: "auth.json",
  defaultLabel: "Codex",
};

const GEMINI_DISCOVERY: ProviderDiscoverySpec = {
  provider: "gemini",
  configDirEnvVar: "GEMINI_CONFIG_DIR",
  defaultDirectoryName: ".gemini",
  credentialsFileName: "oauth_creds.json",
  defaultLabel: "Gemini",
};

/**
 * 조회를 시도할 config dir 후보를 순서대로 모은다.
 *
 * 환경변수로 지정한 디렉터리를 가장 신뢰하고, 그다음이 홈의 기본 디렉터리다.
 * 계정을 여러 개 쓰는 사람은 `~/.claude-work`처럼 접두사가 같은 형제 디렉터리를 만들어
 * 환경변수로 갈아 끼우는 관행을 쓰므로, 그 형제들도 후보로 넣는다.
 */
async function collectConfigDirCandidates(spec: ProviderDiscoverySpec): Promise<string[]> {
  const homeDirectory = homedir();
  const candidates: string[] = [];

  const configuredDir = process.env[spec.configDirEnvVar]?.trim();
  if (configuredDir) {
    candidates.push(path.resolve(configuredDir));
  }

  candidates.push(path.join(homeDirectory, spec.defaultDirectoryName));

  try {
    const homeEntries = await readdir(homeDirectory, { withFileTypes: true });
    for (const entry of homeEntries) {
      if (entry.isDirectory() && entry.name.startsWith(`${spec.defaultDirectoryName}-`)) {
        candidates.push(path.join(homeDirectory, entry.name));
      }
    }
  } catch {
    // 홈을 못 읽으면 기본 후보만으로 진행한다
  }

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
 * Claude는 사용량 응답에 계정 정보를 담지 않아서 `.claude.json`이 유일한 식별·라벨 출처다.
 * config dir를 따로 지정한 계정은 그 안에 `.claude.json`을 함께 두고, 기본 계정만 홈 루트에 둔다.
 */
async function readClaudeAccountIdentity(configDir: string): Promise<{
  accountId: string | null;
  label: string | null;
}> {
  const colocatedConfig = await readJsonFile(path.join(configDir, ".claude.json"));
  const config = colocatedConfig ?? (await readJsonFile(path.join(homedir(), ".claude.json")));
  const oauthAccount = config?.oauthAccount as Record<string, unknown> | undefined;

  const accountUuid = oauthAccount?.accountUuid;
  const emailAddress = oauthAccount?.emailAddress;
  const displayName = oauthAccount?.displayName;

  return {
    accountId: typeof accountUuid === "string" && accountUuid.trim() ? accountUuid : null,
    label: firstNonEmptyString([emailAddress, displayName]),
  };
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
  const configDir = path.join(homedir(), CLAUDE_DISCOVERY.defaultDirectoryName);
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
    label: identity.label ?? CLAUDE_DISCOVERY.defaultLabel,
    configDir,
  };
}

export async function discoverClaudeAccounts(): Promise<AiUsageAccount[]> {
  const candidates = await collectConfigDirCandidates(CLAUDE_DISCOVERY);
  const accounts: AiUsageAccount[] = [];

  for (const configDir of candidates) {
    const credentials = await readJsonFile(path.join(configDir, CLAUDE_DISCOVERY.credentialsFileName));
    const oauth = credentials?.claudeAiOauth as Record<string, unknown> | undefined;
    if (!firstNonEmptyString([oauth?.accessToken])) {
      continue;
    }

    const identity = await readClaudeAccountIdentity(configDir);
    accounts.push({
      provider: "claude",
      accountId: identity.accountId ?? configDir,
      label: identity.label ?? CLAUDE_DISCOVERY.defaultLabel,
      configDir,
    });
  }

  // 파일로 찾은 계정이 있으면 Keychain 승인 프롬프트를 띄울 이유가 없다
  if (accounts.length === 0) {
    const keychainAccount = await discoverClaudeKeychainAccount(candidates);
    if (keychainAccount) {
      accounts.push(keychainAccount);
    }
  }

  return deduplicateAccounts(accounts);
}

export async function discoverCodexAccounts(): Promise<AiUsageAccount[]> {
  const candidates = await collectConfigDirCandidates(CODEX_DISCOVERY);
  const accounts: AiUsageAccount[] = [];

  for (const configDir of candidates) {
    const auth = await readJsonFile(path.join(configDir, CODEX_DISCOVERY.credentialsFileName));
    const tokens = auth?.tokens as Record<string, unknown> | undefined;
    if (!firstNonEmptyString([tokens?.access_token])) {
      continue;
    }

    const accountId = firstNonEmptyString([tokens?.account_id]);
    accounts.push({
      provider: "codex",
      accountId: accountId ?? configDir,
      label: readEmailFromIdToken(tokens?.id_token) ?? CODEX_DISCOVERY.defaultLabel,
      configDir,
    });
  }

  return deduplicateAccounts(accounts);
}

export async function discoverGeminiAccounts(): Promise<AiUsageAccount[]> {
  const candidates = await collectConfigDirCandidates(GEMINI_DISCOVERY);
  const accounts: AiUsageAccount[] = [];

  for (const configDir of candidates) {
    const credentials = await readJsonFile(path.join(configDir, GEMINI_DISCOVERY.credentialsFileName));
    if (!firstNonEmptyString([credentials?.access_token])) {
      continue;
    }

    accounts.push({
      provider: "gemini",
      accountId: configDir,
      label: GEMINI_DISCOVERY.defaultLabel,
      configDir,
    });
  }

  return deduplicateAccounts(accounts);
}
