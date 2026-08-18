import { homedir } from "os";
import path from "path";
import type { AiUsageProvider } from "@/lib/aiUsage/types";

/**
 * 계정 하나가 디스크의 어디에 놓이고 그 위치를 CLI에 어떻게 알리는지를 provider마다 한곳에 모은다.
 *
 * 탐색과 로그인과 계정 생성이 각자 경로 규칙을 들고 있으면 한쪽만 틀린 채로 남는다.
 * Gemini의 config dir 환경변수가 실제로는 존재하지 않는데도 오래 남아 있던 것이 그 사례다.
 */
export interface AiProviderConfigDirSpec {
  provider: AiUsageProvider;
  /** CLI에 계정 위치를 알릴 때 쓰는 환경변수 */
  homeEnvVar: string;
  /** 기본 계정의 루트를 홈 기준으로 적은 이름. 빈 문자열이면 홈 자체가 루트다 */
  defaultRootName: string;
  /** 계정을 나눠 쓰는 형제 루트를 알아보는 접두사 */
  siblingRootPrefix: string;
  /** 루트 아래에서 자격증명이 실제로 놓이는 하위 경로. 루트가 곧 config dir면 빈 문자열이다 */
  configDirSubPath: string;
  credentialsFileName: string;
  defaultLabel: string;
}

const CLAUDE_CONFIG_DIR_SPEC: AiProviderConfigDirSpec = {
  provider: "claude",
  homeEnvVar: "CLAUDE_CONFIG_DIR",
  defaultRootName: ".claude",
  siblingRootPrefix: ".claude-",
  configDirSubPath: "",
  credentialsFileName: ".credentials.json",
  defaultLabel: "Claude",
};

const CODEX_CONFIG_DIR_SPEC: AiProviderConfigDirSpec = {
  provider: "codex",
  homeEnvVar: "CODEX_HOME",
  defaultRootName: ".codex",
  siblingRootPrefix: ".codex-",
  configDirSubPath: "",
  credentialsFileName: "auth.json",
  defaultLabel: "Codex",
};

/**
 * Gemini CLI는 config dir를 직접 받지 않는다. `GEMINI_CLI_HOME`은 루트를 받고 그 아래에 `.gemini`를 만든다.
 * 그래서 기본 계정의 루트는 홈 자체이고, 계정을 나눈 루트도 그 아래에 `.gemini`를 한 겹 더 가진다.
 */
const GEMINI_CONFIG_DIR_SPEC: AiProviderConfigDirSpec = {
  provider: "gemini",
  homeEnvVar: "GEMINI_CLI_HOME",
  defaultRootName: "",
  siblingRootPrefix: ".gemini-",
  configDirSubPath: ".gemini",
  credentialsFileName: "oauth_creds.json",
  defaultLabel: "Gemini",
};

export const AI_PROVIDER_CONFIG_DIR_SPECS: Record<AiUsageProvider, AiProviderConfigDirSpec> = {
  claude: CLAUDE_CONFIG_DIR_SPEC,
  codex: CODEX_CONFIG_DIR_SPEC,
  gemini: GEMINI_CONFIG_DIR_SPEC,
};

/** 자격증명이 놓이는 디렉터리. 루트가 곧 config dir인 provider는 루트를 그대로 돌려준다 */
export function toAccountConfigDir(spec: AiProviderConfigDirSpec, accountRoot: string): string {
  return spec.configDirSubPath ? path.join(accountRoot, spec.configDirSubPath) : accountRoot;
}

/** 기본 계정의 루트 */
export function toDefaultAccountRoot(
  spec: AiProviderConfigDirSpec,
  homeDirectory: string = homedir(),
): string {
  return spec.defaultRootName ? path.join(homeDirectory, spec.defaultRootName) : homeDirectory;
}

/** 사용자가 붙인 계정 이름으로 형제 루트 경로를 만든다 */
export function toNamedAccountRoot(
  spec: AiProviderConfigDirSpec,
  accountName: string,
  homeDirectory: string = homedir(),
): string {
  return path.join(homeDirectory, `${spec.siblingRootPrefix}${accountName}`);
}

/** 형제 루트에서 사용자가 붙였던 계정 이름을 되읽는다. 기본 계정 루트면 null */
export function toAccountNameFromRoot(
  spec: AiProviderConfigDirSpec,
  accountRoot: string,
): string | null {
  const rootName = path.basename(accountRoot);
  return rootName.startsWith(spec.siblingRootPrefix)
    ? rootName.slice(spec.siblingRootPrefix.length) || null
    : null;
}

/**
 * 계정 이름은 홈 아래 디렉터리 이름이 되므로 경로를 벗어날 수 있는 글자를 받지 않는다.
 * 거절해야 할 값을 통과시키면 홈 밖에 디렉터리를 만들게 된다.
 */
export function isValidAccountName(accountName: string): boolean {
  return /^[A-Za-z0-9._-]{1,40}$/.test(accountName) && !accountName.startsWith(".");
}
