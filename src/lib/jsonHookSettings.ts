/**
 * Claude Code / Gemini CLI / Codex CLI가 공통으로 사용하는 JSON hook 설정 파일 조작 유틸리티.
 * 세 CLI 모두 `{ hooks: { <이벤트>: [ { matcher?, hooks: [{ type, command, timeout }] } ] } }`
 * 구조를 사용하므로 파싱·upsert·검증 규칙을 한 곳에서 관리한다.
 */

export interface CommandHookConfig {
  type: string;
  command: string;
  timeout: number;
}

export interface CommandHookEntry {
  hooks: CommandHookConfig[];
}

export interface MatcherCommandHookEntry extends CommandHookEntry {
  matcher: string;
}

export interface JsonHookSettings {
  hooks?: Record<string, unknown[]>;
  [key: string]: unknown;
}

export function parseJsonHookSettings(content: string): JsonHookSettings {
  if (!content) {
    return {};
  }

  try {
    return JSON.parse(content) as JsonHookSettings;
  } catch {
    return {};
  }
}

export function serializeJsonHookSettings(settings: JsonHookSettings): string {
  return JSON.stringify(settings, null, 2) + "\n";
}

/** settings 객체의 hooks bucket 맵을 보장하고 반환한다 */
export function ensureJsonHookBuckets(settings: JsonHookSettings): Record<string, unknown[]> {
  if (!settings.hooks) {
    settings.hooks = {};
  }

  return settings.hooks;
}

export function buildCommandHookEntry(command: string, timeout: number): CommandHookEntry {
  return { hooks: [{ type: "command", command, timeout }] };
}

export function buildMatcherCommandHookEntry(
  matcher: string,
  command: string,
  timeout: number,
): MatcherCommandHookEntry {
  return { matcher, ...buildCommandHookEntry(command, timeout) };
}

/** 같은 스크립트를 가리키는 기존 entry만 교체하고 사용자가 추가한 다른 entry는 보존한다 */
export function upsertJsonHookEntries<T>(
  hookEntries: unknown[] | undefined,
  scriptName: string,
  nextEntry: T,
): T[] {
  const preservedEntries = Array.isArray(hookEntries)
    ? hookEntries.filter((entry) => !referencesScriptName(entry, scriptName)) as T[]
    : [];
  preservedEntries.push(nextEntry);
  return preservedEntries;
}

export function hasCommandHookEntry(hookEntries: unknown[], command: string): boolean {
  if (!Array.isArray(hookEntries)) return false;
  return hookEntries.some((entry) => {
    const typed = entry as CommandHookEntry;
    return typed.hooks?.some((hook) => hook.type === "command" && hook.command === command);
  });
}

export function hasMatcherCommandHookEntry(
  hookEntries: unknown[],
  matcher: string,
  command: string,
): boolean {
  if (!Array.isArray(hookEntries)) return false;
  return hookEntries.some((entry) => {
    const typed = entry as MatcherCommandHookEntry;
    return typed.matcher === matcher
      && typed.hooks?.some((hook) => hook.type === "command" && hook.command === command);
  });
}

function referencesScriptName(entry: unknown, scriptName: string): boolean {
  return JSON.stringify(entry).includes(scriptName);
}
