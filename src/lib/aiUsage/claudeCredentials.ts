import { execFile } from "node:child_process";
import { createHash } from "node:crypto";
import { userInfo } from "node:os";

/** Claude Code가 macOS Keychain에 자격증명을 넣을 때 쓰는 서비스 이름 */
const CLAUDE_KEYCHAIN_SERVICE = "Claude Code-credentials";
const CLAUDE_KEYCHAIN_CONFIG_HASH_LENGTH = 8;

/**
 * Finder나 Dock에서 띄운 앱은 로그인 셸의 PATH를 물려받지 못한다.
 * 이름만 넘기면 실행 자체가 실패할 수 있어 시스템 도구를 절대경로로 부른다.
 */
const SECURITY_COMMAND_PATH = "/usr/bin/security";

/**
 * Keychain 조회는 접근 승인 프롬프트에서 사용자를 기다릴 수 있다.
 * 사용량 패널이 그 대기에 묶이지 않도록 상한을 둔다.
 */
const KEYCHAIN_READ_TIMEOUT_MS = 10_000;

const KEYCHAIN_OUTPUT_MAX_BYTES = 1024 * 1024;

function readClaudeOauthSection(credentialsJson: string): Record<string, unknown> | null {
  if (!credentialsJson) {
    return null;
  }

  try {
    const parsed = JSON.parse(credentialsJson) as { claudeAiOauth?: Record<string, unknown> };
    return parsed?.claudeAiOauth ?? null;
  } catch {
    return null;
  }
}

function readNonEmptyString(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

export function readClaudeAccessToken(credentialsJson: string): string | null {
  return readNonEmptyString(readClaudeOauthSection(credentialsJson)?.accessToken);
}

/** 구독 등급 표시값. 사용량 응답에는 계정 정보가 없어 자격증명이 유일한 출처다 */
export function readClaudeSubscriptionType(credentialsJson: string): string | null {
  return readNonEmptyString(readClaudeOauthSection(credentialsJson)?.subscriptionType);
}

/** `security(1)`가 항목을 찾지 못했을 때 내는 종료 상태 */
const SECURITY_ITEM_NOT_FOUND_EXIT_CODE = 44;

/**
 * Keychain 조회 결과.
 *
 * "로그인하지 않음"과 "읽지 못함"을 가르는 것이 이 타입의 존재 이유다.
 * 둘을 뭉치면 잠긴 Keychain 때문에 읽지 못한 사용자에게 재로그인을 권하게 된다.
 */
export type ClaudeKeychainReadResult =
  | { outcome: "found"; credentials: string }
  | { outcome: "absent" }
  | { outcome: "unreadable" };

type SecurityReadResult =
  | { outcome: "found"; output: string }
  | { outcome: "absent" }
  | { outcome: "unreadable" };

/**
 * 네이티브 Keychain API가 아니라 `security`를 실행하는 것은 의도된 선택이다.
 * Claude Code가 항목을 만들 때 접근 제어를 지정하지 않아 ACL의 신뢰 앱이 `/usr/bin/security` 하나뿐이라,
 * 같은 도구로 읽으면 승인 창 없이 통과한다. 앱에서 직접 API를 부르면 매번 승인 창이 뜬다.
 *
 * 출력에는 토큰이 들어 있으므로 성공·실패 어느 쪽도 로그로 남기지 않는다.
 */
function runSecurity(args: string[]): Promise<SecurityReadResult> {
  return new Promise((resolve) => {
    execFile(
      SECURITY_COMMAND_PATH,
      args,
      { timeout: KEYCHAIN_READ_TIMEOUT_MS, maxBuffer: KEYCHAIN_OUTPUT_MAX_BYTES },
      (error, stdout) => {
        if (!error) {
          resolve({ outcome: "found", output: stdout.trim() });
          return;
        }

        // 사유를 모르는 실패는 "읽지 못함"으로 둔다. 로그인 여부를 단정하지 않는 쪽이 안전하다
        const exitCode = (error as { code?: unknown }).code;
        resolve(exitCode === SECURITY_ITEM_NOT_FOUND_EXIT_CODE
          ? { outcome: "absent" }
          : { outcome: "unreadable" });
      },
    );
  });
}

/**
 * 서비스 하나를 현재 사용자 계정으로 먼저 찾고, 없으면 계정을 지정하지 않고 한 번 더 찾는다.
 * 항목의 계정명이 OS 사용자명과 다르게 저장된 사례가 있어, 계정을 가정하면 있는 항목을 놓친다.
 */
async function readServiceCredentials(service: string): Promise<SecurityReadResult> {
  const accountScopedResult = await runSecurity([
    "find-generic-password",
    "-a",
    userInfo().username,
    "-s",
    service,
    "-w",
  ]);
  if (accountScopedResult.outcome !== "absent") {
    return accountScopedResult;
  }

  return runSecurity(["find-generic-password", "-s", service, "-w"]);
}

function toScopedService(configDir: string): string {
  const configHash = createHash("sha256")
    .update(configDir)
    .digest("hex")
    .slice(0, CLAUDE_KEYCHAIN_CONFIG_HASH_LENGTH);
  return `${CLAUDE_KEYCHAIN_SERVICE}-${configHash}`;
}

/** Claude Code 2.1+는 config dir로 스코프한 서비스에 쓰고, 그 이전 버전은 접미사 없는 서비스에 썼다 */
function buildGuessedServices(configDirs: string[]): string[] {
  return [...new Set([...configDirs.map(toScopedService), CLAUDE_KEYCHAIN_SERVICE])];
}

/**
 * 추측한 서비스 이름이 모두 빗나갔을 때 Keychain에 실제로 있는 이름을 훑는다.
 *
 * 스코프 해시는 Claude Code가 실행될 때의 config dir로 만들어지는데, GUI에서 띄운 앱은
 * 사용자가 셸에 설정한 `CLAUDE_CONFIG_DIR`를 물려받지 못해 그 값을 알 수 없다.
 * `-d` 없이 실행하면 비밀값을 복호화하지 않아 승인 프롬프트 없이 이름만 나온다.
 */
async function listStoredClaudeServices(): Promise<string[]> {
  const dumpResult = await runSecurity(["dump-keychain"]);
  if (dumpResult.outcome !== "found") {
    return [];
  }

  const storedServices = [...dumpResult.output.matchAll(/"svce"<blob>="([^"]*)"/g)]
    .map(([, service]) => service)
    .filter((service) => service.startsWith(CLAUDE_KEYCHAIN_SERVICE));
  return [...new Set(storedServices)];
}

interface KeychainServiceAttempt {
  service: string;
  outcome: SecurityReadResult["outcome"];
}

interface ClaudeKeychainSearchResult {
  credentials: string | null;
  attempts: KeychainServiceAttempt[];
}

/**
 * 로그인 토큰이 실제로 들어 있는 첫 항목을 고른다.
 *
 * Claude Code 2.1.x는 접미사 없는 항목에 MCP 서버 토큰만 남기는 경우가 있어,
 * "항목을 찾았다"에서 멈추면 정작 로그인 토큰을 가진 뒤 후보를 놓친다.
 */
async function findCredentialsWithAccessToken(
  services: string[],
): Promise<ClaudeKeychainSearchResult> {
  const attempts: KeychainServiceAttempt[] = [];

  for (const service of services) {
    const result = await readServiceCredentials(service);
    attempts.push({ service, outcome: result.outcome });

    if (result.outcome === "found" && readClaudeAccessToken(result.output)) {
      return { credentials: result.output, attempts };
    }
  }

  return { credentials: null, attempts };
}

/** 맥에서만 나는 실패라 로그가 유일한 단서다. 항목 값에는 토큰이 들어 있으므로 이름과 결과만 남긴다 */
function reportKeychainMiss(attempts: KeychainServiceAttempt[]): void {
  const attemptSummary = attempts
    .map(({ service, outcome }) => `${service}=${outcome}`)
    .join(", ");
  console.warn(`[ai-usage] Keychain에서 Claude 로그인 토큰을 찾지 못했습니다: ${attemptSummary}`);
}

/**
 * macOS의 Claude Code는 자격증명을 Keychain에 두고 `.credentials.json`은 폴백으로만 쓴다.
 * 그래서 config dir에 파일이 하나도 없어도 로그인되어 있을 수 있다.
 *
 * 이름을 추측해 먼저 찾고, 모두 빗나갔을 때만 실제 항목 목록을 훑는다.
 * 목록 조회는 값이 아니라 이름만 읽어도 되는 마지막 수단이라 평소 경로의 비용으로 두지 않는다.
 */
export async function readClaudeKeychainCredentials(
  configDirs: string[],
): Promise<ClaudeKeychainReadResult> {
  if (process.platform !== "darwin") {
    return { outcome: "absent" };
  }

  const guessedServices = buildGuessedServices(configDirs);
  const guessedSearch = await findCredentialsWithAccessToken(guessedServices);
  if (guessedSearch.credentials) {
    return { outcome: "found", credentials: guessedSearch.credentials };
  }

  const storedServices = (await listStoredClaudeServices())
    .filter((service) => !guessedServices.includes(service));
  const storedSearch = await findCredentialsWithAccessToken(storedServices);
  if (storedSearch.credentials) {
    return { outcome: "found", credentials: storedSearch.credentials };
  }

  const attempts = [...guessedSearch.attempts, ...storedSearch.attempts];
  reportKeychainMiss(attempts);
  return attempts.some(({ outcome }) => outcome === "unreadable")
    ? { outcome: "unreadable" }
    : { outcome: "absent" };
}
