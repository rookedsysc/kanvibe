import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { createLocalShellEnvironment } from "@/lib/shellEnvironment";

const execFileAsync = promisify(execFile);
const REGISTRATION_CACHE_TTL_MS = 5000;
const DEBUG_CONFIG_TIMEOUT_MS = 3000;
const registrationQueries = new Map<string, {
  promise: Promise<string[]>;
  expiresAt: number;
  invalidated: boolean;
}>();

export function invalidateOpenCodePluginRegistration(repoPath: string): void {
  const query = registrationQueries.get(resolve(repoPath));
  if (query) query.invalidated = true;
}

interface OpenCodeDebugConfig {
  plugin?: unknown;
}

function normalizePluginEntries(values: unknown): string[] {
  if (!Array.isArray(values)) {
    return [];
  }

  return values.filter((value): value is string => typeof value === "string");
}

export function extractRegisteredPluginUrls(output: string): string[] {
  try {
    const config = JSON.parse(output) as OpenCodeDebugConfig;
    const pluginEntries = normalizePluginEntries(config.plugin);
    if (pluginEntries.length > 0) {
      return pluginEntries;
    }
  } catch {
    // OpenCode debug output can embed invalid JSON in later sections. Fall back to the top-level plugin block.
  }

  const pluginBlockMatch = output.match(/"plugin"\s*:\s*\[([\s\S]*?)\]\s*,\s*"[^"]+"\s*:/);
  const pluginBlock = pluginBlockMatch?.[1];
  if (!pluginBlock) {
    return [];
  }

  return Array.from(pluginBlock.matchAll(/"((?:\\.|[^"\\])*)"/g), ([, value]) => JSON.parse(`"${value}"`) as string);
}

export function isKanvibePluginUrl(value: string): boolean {
  return /\/kanvibe-plugin\.(?:[cm]?js|ts)$/i.test(value);
}

export function extractRegisteredKanvibePluginUrls(output: string): string[] {
  return extractRegisteredPluginUrls(output).filter(isKanvibePluginUrl);
}

export async function getOpenCodeRegisteredKanvibePluginUrls(repoPath: string): Promise<string[]> {
  const key = resolve(repoPath);
  const existing = registrationQueries.get(key);
  if (existing?.invalidated) {
    // 설치 이전에 시작한 조회가 끝난 뒤 새 조회를 공유해 동시 CLI 실행을 막는다.
    await existing.promise.catch(() => undefined);
    if (registrationQueries.get(key) === existing) registrationQueries.delete(key);
    return getOpenCodeRegisteredKanvibePluginUrls(key);
  }
  if (existing && existing.expiresAt > Date.now()) {
    return [...await existing.promise];
  }

  for (const [path, query] of registrationQueries) {
    if (query.expiresAt <= Date.now()) registrationQueries.delete(path);
  }

  const query = {
    promise: execFileAsync("opencode", ["debug", "config"], {
      cwd: key,
      env: createLocalShellEnvironment(),
      maxBuffer: 1024 * 1024,
      timeout: DEBUG_CONFIG_TIMEOUT_MS,
      killSignal: "SIGKILL",
    }).then(({ stdout }) => extractRegisteredKanvibePluginUrls(stdout)).finally(() => {
      query.expiresAt = Date.now() + REGISTRATION_CACHE_TTL_MS;
    }),
    expiresAt: Infinity,
    invalidated: false,
  };
  registrationQueries.set(key, query);
  return [...await query.promise];
}

export async function isOpenCodePluginRegistered(repoPath: string, pluginPath: string): Promise<boolean> {
  try {
    const expectedPluginUrl = pathToFileURL(pluginPath).href;
    return (await getOpenCodeRegisteredKanvibePluginUrls(repoPath)).some((value) => value === expectedPluginUrl);
  } catch {
    return false;
  }
}
