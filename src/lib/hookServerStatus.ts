import { execGit } from "@/lib/gitOperations";
import { quoteShellArgument } from "@/lib/hostFileAccess";
import { getHookServerToken, getHookServerUrl } from "@/lib/hookEndpoint";

export interface HookServerValidation {
  hasExpectedHookServerUrl: boolean;
  hasReachableHookServer: boolean;
  expectedHookServerUrl: string | null;
  configuredHookServerUrl: string | null;
}

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
    expectedHookServerUrl = await getHookServerUrl(sshHost);
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

  if (!sshHost) {
    try {
      const response = await fetch(healthCheckUrl, {
        headers: buildHealthCheckHeaders(),
      });
      return response.ok;
    } catch {
      return false;
    }
  }

  try {
    const token = getHookServerToken();
    const headerArgs = token.length > 0
      ? ` -H ${quoteShellArgument(`X-Kanvibe-Token: ${token}`)}`
      : "";

    await execGit(
      `command -v curl >/dev/null 2>&1 && curl -fsS --max-time 2${headerArgs} ${quoteShellArgument(healthCheckUrl)} >/dev/null`,
      sshHost,
    );
    return true;
  } catch {
    return false;
  }
}

function buildHealthCheckHeaders() {
  const token = getHookServerToken();
  return token.length > 0 ? { "X-Kanvibe-Token": token } : undefined;
}

function ensureTrailingSlash(url: string) {
  return url.endsWith("/") ? url : `${url}/`;
}
