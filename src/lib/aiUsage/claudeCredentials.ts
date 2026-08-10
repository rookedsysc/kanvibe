import { execFile } from "node:child_process";
import { createHash } from "node:crypto";
import { userInfo } from "node:os";

/** Claude Code가 macOS Keychain에 자격증명을 넣을 때 쓰는 서비스 이름 */
const CLAUDE_KEYCHAIN_SERVICE = "Claude Code-credentials";
const CLAUDE_KEYCHAIN_CONFIG_HASH_LENGTH = 8;

/**
 * Keychain 조회는 접근 승인 프롬프트에서 사용자를 기다릴 수 있다.
 * 사용량 패널이 그 대기에 묶이지 않도록 상한을 둔다.
 */
const KEYCHAIN_READ_TIMEOUT_MS = 10_000;

const KEYCHAIN_OUTPUT_MAX_BYTES = 1024 * 1024;

export function readClaudeAccessToken(credentialsJson: string): string | null {
  if (!credentialsJson) {
    return null;
  }

  try {
    const parsed = JSON.parse(credentialsJson) as { claudeAiOauth?: { accessToken?: unknown } };
    const accessToken = parsed?.claudeAiOauth?.accessToken;
    return typeof accessToken === "string" && accessToken.trim() ? accessToken : null;
  } catch {
    return null;
  }
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

/**
 * macOS의 Claude Code는 자격증명을 Keychain에 두고 `.credentials.json`은 폴백으로만 쓴다.
 * 그래서 파일이 없어도 로그인되어 있을 수 있다.
 *
 * 네이티브 Keychain API가 아니라 `security`를 실행하는 것은 의도된 선택이다.
 * Claude Code가 항목을 만들 때 접근 제어를 지정하지 않아 ACL의 신뢰 앱이 `/usr/bin/security` 하나뿐이라,
 * 같은 도구로 읽으면 승인 창 없이 통과한다. 앱에서 직접 API를 부르면 매번 승인 창이 뜬다.
 *
 * 출력에는 토큰이 들어 있으므로 성공·실패 어느 쪽도 로그로 남기지 않는다.
 */
function getClaudeKeychainServices(configDir?: string): string[] {
  if (!configDir) {
    return [CLAUDE_KEYCHAIN_SERVICE];
  }

  const configHash = createHash("sha256")
    .update(configDir)
    .digest("hex")
    .slice(0, CLAUDE_KEYCHAIN_CONFIG_HASH_LENGTH);
  return [`${CLAUDE_KEYCHAIN_SERVICE}-${configHash}`, CLAUDE_KEYCHAIN_SERVICE];
}

function readKeychainServiceCredentials(service: string): Promise<ClaudeKeychainReadResult> {
  const keychainArgs = [
    "find-generic-password",
    "-a",
    userInfo().username,
    "-s",
    service,
    "-w",
  ];

  return new Promise((resolve) => {
    execFile(
      "security",
      keychainArgs,
      { timeout: KEYCHAIN_READ_TIMEOUT_MS, maxBuffer: KEYCHAIN_OUTPUT_MAX_BYTES },
      (error, stdout) => {
        if (!error) {
          resolve({ outcome: "found", credentials: stdout.trim() });
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

export async function readClaudeKeychainCredentials(
  configDir?: string,
): Promise<ClaudeKeychainReadResult> {
  if (process.platform !== "darwin") {
    return { outcome: "absent" };
  }

  for (const service of getClaudeKeychainServices(configDir)) {
    const result = await readKeychainServiceCredentials(service);
    if (result.outcome !== "absent") {
      return result;
    }
  }

  return { outcome: "absent" };
}
