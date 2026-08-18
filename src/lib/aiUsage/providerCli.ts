import { execFile } from "node:child_process";
import {
  AI_PROVIDER_CONFIG_DIR_SPECS,
  type AiProviderConfigDirSpec,
} from "@/lib/aiUsage/providerConfigDir";
import { createLocalShellEnvironment } from "@/lib/shellEnvironment";
import type { AiUsageProvider } from "@/lib/aiUsage/types";

/** 상태 조회는 토큰 갱신을 위해 네트워크를 한 번 타므로 사용량 조회보다 넉넉히 준다 */
const PROVIDER_CLI_TIMEOUT_MS = 30_000;

const PROVIDER_CLI_OUTPUT_MAX_BYTES = 1024 * 1024;

interface ProviderCliResult {
  /** 실행 자체가 불가능하면 null. 바이너리를 찾지 못했거나 실행 권한이 없는 경우다 */
  exitCode: number | null;
  stdout: string;
  stderr: string;
}

export interface ProviderAuthStatus {
  isLoggedIn: boolean;
  /** 계정을 알아볼 표시 이름. CLI가 알려줄 때만 채운다 */
  label: string | null;
  /** 구독 등급 표시값. CLI가 알려줄 때만 채운다 */
  planName: string | null;
}

interface AiProviderCliSpec {
  command: string;
  loginArgs: string[];
  /** 저장된 자격증명을 지우는 명령을 가진 provider만 채운다 */
  logoutArgs: string[] | null;
  /** 로그인 여부를 물을 수 있는 provider만 채운다 */
  statusArgs: string[] | null;
  parseStatus: (stdout: string) => ProviderAuthStatus | null;
}

function readNonEmptyString(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

/** `claude auth status --json`은 로그인 여부와 계정 이메일과 구독 등급을 한 번에 준다 */
function parseClaudeAuthStatus(stdout: string): ProviderAuthStatus | null {
  try {
    const parsed = JSON.parse(stdout) as {
      loggedIn?: unknown;
      email?: unknown;
      subscriptionType?: unknown;
    };
    return {
      isLoggedIn: parsed.loggedIn === true,
      label: readNonEmptyString(parsed.email),
      planName: readNonEmptyString(parsed.subscriptionType),
    };
  } catch {
    return null;
  }
}

/** `codex login status`는 사람이 읽는 한 줄만 준다. 등급과 계정은 사용량 응답 쪽이 알려준다 */
function parseCodexAuthStatus(stdout: string): ProviderAuthStatus | null {
  const output = stdout.toLowerCase();
  if (!output.includes("logged in") && !output.includes("not logged in")) {
    return null;
  }

  return {
    isLoggedIn: output.includes("logged in") && !output.includes("not logged in"),
    label: null,
    planName: null,
  };
}

/**
 * Gemini CLI에는 로그인 여부를 물을 수 있는 하위 명령이 없고, 로그인도 첫 실행 화면에서 고르는 방식이다.
 * 그래서 상태는 자격증명 파일로만 판단하고 로그인은 CLI를 그대로 띄워 사용자가 고르게 한다.
 */
const AI_PROVIDER_CLI_SPECS: Record<AiUsageProvider, AiProviderCliSpec> = {
  claude: {
    command: "claude",
    loginArgs: ["auth", "login", "--claudeai"],
    logoutArgs: ["auth", "logout"],
    statusArgs: ["auth", "status", "--json"],
    parseStatus: parseClaudeAuthStatus,
  },
  codex: {
    command: "codex",
    loginArgs: ["login"],
    logoutArgs: ["logout"],
    statusArgs: ["login", "status"],
    parseStatus: parseCodexAuthStatus,
  },
  gemini: {
    command: "gemini",
    loginArgs: [],
    logoutArgs: null,
    statusArgs: null,
    parseStatus: () => null,
  },
};

/**
 * 계정 위치를 알리는 변수 하나만 얹은 자식 프로세스 환경을 만든다.
 *
 * 서버 런타임 값이 CLI로 새지 않도록 로컬 셸 환경 규칙을 그대로 쓰고, 그 위에 provider 변수만 더한다.
 */
export function createProviderCliEnvironment(
  spec: AiProviderConfigDirSpec,
  accountRoot: string,
): Record<string, string> {
  return { ...createLocalShellEnvironment(), [spec.homeEnvVar]: accountRoot };
}

/** 로그인 세션이 띄울 명령. PTY를 다루는 쪽이 실행 방식을 정하므로 여기서는 이름과 인자만 준다 */
export function getProviderLoginCommand(provider: AiUsageProvider): {
  command: string;
  args: string[];
} {
  const { command, loginArgs } = AI_PROVIDER_CLI_SPECS[provider];
  return { command, args: loginArgs };
}

function runProviderCli(
  provider: AiUsageProvider,
  accountRoot: string,
  args: string[],
): Promise<ProviderCliResult> {
  const { command } = AI_PROVIDER_CLI_SPECS[provider];
  const environment = createProviderCliEnvironment(
    AI_PROVIDER_CONFIG_DIR_SPECS[provider],
    accountRoot,
  );

  return new Promise((resolve) => {
    execFile(
      command,
      args,
      {
        env: environment,
        timeout: PROVIDER_CLI_TIMEOUT_MS,
        maxBuffer: PROVIDER_CLI_OUTPUT_MAX_BYTES,
      },
      (error, stdout, stderr) => {
        const exitCode = error ? (error as { code?: unknown }).code : 0;
        resolve({
          exitCode: typeof exitCode === "number" ? exitCode : error ? null : 0,
          stdout: stdout ?? "",
          stderr: stderr ?? "",
        });
      },
    );
  });
}

/**
 * CLI에게 이 계정의 로그인 상태를 묻는다.
 *
 * 물을 수 없는 provider이거나 CLI를 실행하지 못하면 null이다. null은 "로그아웃"이 아니라 "모른다"이므로,
 * 호출부는 자격증명 파일로 판단한 결과를 null로 덮어쓰지 않아야 한다.
 */
export async function readProviderAuthStatus(
  provider: AiUsageProvider,
  accountRoot: string,
): Promise<ProviderAuthStatus | null> {
  const { statusArgs, parseStatus } = AI_PROVIDER_CLI_SPECS[provider];
  if (!statusArgs) {
    return null;
  }

  const result = await runProviderCli(provider, accountRoot, statusArgs);
  if (result.exitCode === null) {
    return null;
  }

  return parseStatus(result.stdout);
}

/**
 * 저장된 자격증명을 CLI에게 지우게 한다.
 *
 * 지울 명령이 없는 provider는 아무것도 하지 않고 false를 돌려준다.
 * KanVibe가 대신 파일을 지우면 CLI가 아는 상태와 어긋나므로 소유자에게만 맡긴다.
 */
export async function logoutThroughCli(
  provider: AiUsageProvider,
  accountRoot: string,
): Promise<boolean> {
  const { logoutArgs } = AI_PROVIDER_CLI_SPECS[provider];
  if (!logoutArgs) {
    return false;
  }

  const result = await runProviderCli(provider, accountRoot, logoutArgs);
  return result.exitCode === 0;
}

/**
 * 만료 임박한 토큰의 갱신을 CLI에 맡긴다.
 *
 * KanVibe는 벤더 자격증명 저장소에 직접 쓰지 않는다. macOS Keychain 항목은 Claude Code의 소유이고
 * 회전된 refresh 토큰을 KanVibe가 되쓰면 CLI 쪽 로그인이 깨질 수 있어서, 갱신은 소유자에게 맡기고
 * KanVibe는 갱신된 결과를 다시 읽기만 한다.
 *
 * 갱신이 실제로 일어났는지는 알 수 없으므로 돌려주는 값은 "다시 읽어 볼 가치가 있는가"다.
 */
export async function refreshCredentialsThroughCli(
  provider: AiUsageProvider,
  accountRoot: string,
): Promise<boolean> {
  const status = await readProviderAuthStatus(provider, accountRoot);
  return status?.isLoggedIn === true;
}
