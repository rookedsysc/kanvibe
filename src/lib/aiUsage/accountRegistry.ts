import {
  AI_PROVIDER_CONFIG_DIR_SPECS,
  isValidAccountName,
  toNamedAccountRoot,
} from "@/lib/aiUsage/providerConfigDir";
import type { AiUsageProvider } from "@/lib/aiUsage/types";

const KNOWN_PROVIDERS = new Set<string>(["claude", "codex", "gemini"]);

/**
 * 사용자가 KanVibe에서 만든 계정 하나.
 *
 * 탐색만으로는 로그아웃된 계정이 목록에서 사라져 다시 로그인할 자리가 없어진다.
 * 그래서 "만들어 둔 계정"을 따로 기억하고, 탐색 결과와 합쳐 자리를 지킨다.
 */
export interface AiAccountRegistration {
  provider: AiUsageProvider;
  /** CLI에 넘기는 계정 루트. 자격증명 디렉터리는 provider 규칙으로 유도한다 */
  accountRoot: string;
  /** 사용자가 붙인 계정 이름 */
  accountName: string;
}

function isRegistration(value: unknown): value is AiAccountRegistration {
  const registration = value as AiAccountRegistration | null;
  return (
    typeof registration === "object"
    && registration !== null
    && KNOWN_PROVIDERS.has(registration.provider)
    && typeof registration.accountRoot === "string"
    && Boolean(registration.accountRoot)
    && typeof registration.accountName === "string"
    && isValidAccountName(registration.accountName)
  );
}

/**
 * 저장된 계정 목록을 복원한다.
 *
 * 목록이 깨졌다고 사용량 화면 전체를 잃으면 안 되므로, 해석되지 않는 항목은 고쳐 쓰지 않고 버린다.
 */
export function parseAccountRegistrations(raw: string | null): AiAccountRegistration[] {
  if (!raw) {
    return [];
  }

  try {
    const parsed = JSON.parse(raw) as unknown;
    return Array.isArray(parsed) ? parsed.filter(isRegistration) : [];
  } catch {
    return [];
  }
}

export function serializeAccountRegistrations(
  registrations: AiAccountRegistration[],
): string {
  return JSON.stringify(registrations);
}

/** 같은 provider의 같은 이름은 같은 계정이므로 하나만 남긴다 */
export function addAccountRegistration(
  registrations: AiAccountRegistration[],
  provider: AiUsageProvider,
  accountName: string,
  homeDirectory?: string,
): AiAccountRegistration[] {
  const accountRoot = toNamedAccountRoot(
    AI_PROVIDER_CONFIG_DIR_SPECS[provider],
    accountName,
    homeDirectory,
  );
  const others = registrations.filter(
    (registration) => registration.accountRoot !== accountRoot,
  );
  return [...others, { provider, accountRoot, accountName }];
}

export function removeAccountRegistration(
  registrations: AiAccountRegistration[],
  accountRoot: string,
): AiAccountRegistration[] {
  return registrations.filter((registration) => registration.accountRoot !== accountRoot);
}
