import { access, readFile, realpath } from "fs/promises";
import { homedir } from "os";
import path from "path";
import { AI_USAGE_REQUEST_TIMEOUT_MS } from "@/lib/aiUsage/shared";

const GOOGLE_TOKEN_URL = "https://oauth2.googleapis.com/token";
const LOAD_CODE_ASSIST_URL = "https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist";

/**
 * Gemini CLI가 OAuth client 자격을 심어 두는 번들 파일. 설치 방식마다 루트만 다르고 이 아래 경로는 같다.
 */
const OAUTH2_BUNDLE_SUBPATH = path.join(
  "node_modules",
  "@google",
  "gemini-cli-core",
  "dist",
  "src",
  "code_assist",
  "oauth2.js",
);

/** npm 전역·pnpm·Homebrew 설치본에서 번들이 바이너리 기준 몇 단계 위에 있는지의 관측 범위 */
const BUNDLE_SEARCH_DEPTH = 4;

export interface GeminiOAuthClient {
  clientId: string;
  clientSecret: string;
}

export interface GeminiRefreshedToken {
  accessToken: string;
  expiresAt: number;
}

async function fileExists(targetPath: string): Promise<boolean> {
  try {
    await access(targetPath);
    return true;
  } catch {
    return false;
  }
}

/**
 * PATH를 직접 훑고, 그다음 흔한 설치 위치를 본다.
 * GUI로 실행된 앱은 로그인 셸의 PATH를 물려받지 못하는 경우가 있어 PATH만으로는 못 찾는다.
 */
async function findGeminiBinary(): Promise<string | null> {
  const pathDirectories = (process.env.PATH ?? "").split(path.delimiter).filter(Boolean);
  const fallbackDirectories = [
    "/usr/local/bin",
    "/opt/homebrew/bin",
    path.join(homedir(), ".local", "bin"),
    path.join(homedir(), "bin"),
  ];

  for (const directory of [...pathDirectories, ...fallbackDirectories]) {
    const candidate = path.join(directory, "gemini");
    if (await fileExists(candidate)) {
      // Homebrew처럼 bin이 심볼릭 링크면 링크를 풀어야 실제 설치 루트가 나온다
      return realpath(candidate).catch(() => candidate);
    }
  }

  return null;
}

async function findOAuthBundle(geminiBinaryPath: string): Promise<string | null> {
  let ancestor = path.dirname(path.dirname(geminiBinaryPath));

  for (let depth = 0; depth < BUNDLE_SEARCH_DEPTH; depth += 1) {
    const candidate = path.join(ancestor, OAUTH2_BUNDLE_SUBPATH);
    if (await fileExists(candidate)) {
      return candidate;
    }

    const parent = path.dirname(ancestor);
    if (parent === ancestor) {
      break;
    }
    ancestor = parent;
  }

  return null;
}

function parseOAuthClient(bundleSource: string): GeminiOAuthClient | null {
  const clientId = bundleSource.match(/OAUTH_CLIENT_ID\s*=\s*['"]([^'"]+)['"]/)?.[1];
  const clientSecret = bundleSource.match(/OAUTH_CLIENT_SECRET\s*=\s*['"]([^'"]+)['"]/)?.[1];

  return clientId && clientSecret ? { clientId, clientSecret } : null;
}

/**
 * Gemini는 만료된 액세스 토큰을 갱신할 때 CLI가 쓰는 것과 같은 OAuth client를 요구한다.
 * 그 값이 CLI 번들 안에만 있어서 설치본을 찾아 읽는 방법 외에는 갱신할 길이 없다.
 * 설치본이 없거나 번들 형식이 바뀌면 갱신을 포기하고 재로그인을 안내한다.
 */
export async function resolveGeminiOAuthClient(): Promise<GeminiOAuthClient | null> {
  const geminiBinaryPath = await findGeminiBinary();
  if (!geminiBinaryPath) {
    return null;
  }

  const bundlePath = await findOAuthBundle(geminiBinaryPath);
  if (!bundlePath) {
    return null;
  }

  try {
    return parseOAuthClient(await readFile(bundlePath, "utf-8"));
  } catch {
    return null;
  }
}

export async function refreshGeminiAccessToken(
  refreshToken: string,
  client: GeminiOAuthClient,
): Promise<GeminiRefreshedToken | null> {
  try {
    const response = await fetch(GOOGLE_TOKEN_URL, {
      method: "POST",
      headers: { "Content-Type": "application/x-www-form-urlencoded" },
      body: new URLSearchParams({
        client_id: client.clientId,
        client_secret: client.clientSecret,
        refresh_token: refreshToken,
        grant_type: "refresh_token",
      }).toString(),
      signal: AbortSignal.timeout(AI_USAGE_REQUEST_TIMEOUT_MS),
    });

    if (!response.ok) {
      return null;
    }

    const payload = (await response.json()) as { access_token?: unknown; expires_in?: unknown };
    if (typeof payload.access_token !== "string" || !payload.access_token) {
      return null;
    }

    const expiresInSeconds = typeof payload.expires_in === "number" ? payload.expires_in : 0;
    return {
      accessToken: payload.access_token,
      expiresAt: Date.now() + expiresInSeconds * 1000,
    };
  } catch {
    return null;
  }
}

/** 쿼터 조회는 프로젝트 단위라 액세스 토큰만으로는 부족하고 이 호출로 프로젝트를 먼저 알아내야 한다 */
export async function loadGeminiProjectId(accessToken: string): Promise<string | null> {
  try {
    const response = await fetch(LOAD_CODE_ASSIST_URL, {
      method: "POST",
      headers: {
        Authorization: `Bearer ${accessToken}`,
        "Content-Type": "application/json",
      },
      body: JSON.stringify({}),
      signal: AbortSignal.timeout(AI_USAGE_REQUEST_TIMEOUT_MS),
    });

    if (!response.ok) {
      return null;
    }

    const payload = (await response.json()) as { cloudaicompanionProject?: unknown };
    return typeof payload.cloudaicompanionProject === "string" && payload.cloudaicompanionProject
      ? payload.cloudaicompanionProject
      : null;
  } catch {
    return null;
  }
}
