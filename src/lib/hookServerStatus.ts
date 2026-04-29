import { execGit, isSSHTransportError } from "@/lib/gitOperations";
import { quoteShellArgument } from "@/lib/hostFileAccess";
import { getHookServerUrl } from "@/lib/hookEndpoint";

export interface HookServerValidation {
  hasExpectedHookServerUrl: boolean;
  hasReachableHookServer: boolean;
  expectedHookServerUrl: string | null;
  configuredHookServerUrl: string | null;
}

const HOOK_SERVER_CACHE_TTL_MS = process.env.VITEST ? 0 : 5_000;
const expectedHookServerUrlCache = new Map<string, { expiresAt: number; value: Promise<string> }>();
const hookServerReachabilityCache = new Map<string, { expiresAt: number; value: Promise<boolean> }>();

export function extractShellHookServerUrl(content: string): string | null {
  const matched = content.match(/^KANVIBE_URL="([^"]+)"/m);
  return matched?.[1] ?? null;
}

export function extractPluginHookServerUrl(content: string): string | null {
  const matched = content.match(/const KANVIBE_URL = "([^"]+)";/);
  return matched?.[1] ?? null;
}

export async function validateHookServerConfiguration(
  configuredUrls: Array<string | null>,
  shouldValidate: boolean,
  sshHost?: string | null,
): Promise<HookServerValidation> {
  const definedUrls = configuredUrls.filter((value): value is string => typeof value === "string" && value.length > 0);
  const configuredHookServerUrl = definedUrls[0] ?? null;

  if (!shouldValidate) {
    return {
      hasExpectedHookServerUrl: true,
      hasReachableHookServer: true,
      expectedHookServerUrl: null,
      configuredHookServerUrl,
    };
  }

  if (definedUrls.length !== configuredUrls.length || definedUrls.length === 0) {
    return {
      hasExpectedHookServerUrl: false,
      hasReachableHookServer: false,
      expectedHookServerUrl: null,
      configuredHookServerUrl,
    };
  }

  let expectedHookServerUrl: string | null = null;
  try {
    expectedHookServerUrl = await getCachedValue(
      expectedHookServerUrlCache,
      sshHost ?? "local",
      () => getHookServerUrl(sshHost),
    );
  } catch {
    return {
      hasExpectedHookServerUrl: false,
      hasReachableHookServer: false,
      expectedHookServerUrl: null,
      configuredHookServerUrl,
    };
  }

  const hasExpectedHookServerUrl = definedUrls.every((value) => value === expectedHookServerUrl);
  const hasReachableHookServer = hasExpectedHookServerUrl
    ? await isHookServerReachable(expectedHookServerUrl, sshHost)
    : false;

  return {
    hasExpectedHookServerUrl,
    hasReachableHookServer,
    expectedHookServerUrl,
    configuredHookServerUrl,
  };
}

async function isHookServerReachable(baseUrl: string, sshHost?: string | null): Promise<boolean> {
  const healthCheckUrl = new URL("/api/hooks/health", ensureTrailingSlash(baseUrl)).toString();
  return getCachedValue(
    hookServerReachabilityCache,
    `${sshHost ?? "local"}:${healthCheckUrl}`,
    () => performHookServerReachabilityCheck(healthCheckUrl, sshHost),
  );
}

async function performHookServerReachabilityCheck(healthCheckUrl: string, sshHost?: string | null): Promise<boolean> {
  if (!sshHost) {
    try {
      const response = await fetch(healthCheckUrl);
      return response.ok;
    } catch {
      return false;
    }
  }

  try {
    await execGit(
      `command -v curl >/dev/null 2>&1 && curl -fsS --max-time 2 ${quoteShellArgument(healthCheckUrl)} >/dev/null`,
      sshHost,
    );
    return true;
  } catch (error) {
    if (isSSHTransportError(error)) {
      return true;
    }
    return false;
  }
}

async function getCachedValue<T>(
  cache: Map<string, { expiresAt: number; value: Promise<T> }>,
  key: string,
  loader: () => Promise<T>,
): Promise<T> {
  const now = Date.now();
  const cached = cache.get(key);
  if (cached && cached.expiresAt > now) {
    return cached.value;
  }

  const value = loader();
  cache.set(key, {
    expiresAt: now + HOOK_SERVER_CACHE_TTL_MS,
    value,
  });

  try {
    return await value;
  } catch (error) {
    cache.delete(key);
    throw error;
  }
}

function ensureTrailingSlash(url: string) {
  return url.endsWith("/") ? url : `${url}/`;
}
