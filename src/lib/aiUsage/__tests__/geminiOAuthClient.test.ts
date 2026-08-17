import path from "path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { resolveGeminiOAuthClient } from "@/lib/aiUsage/geminiOAuthClient";

const { mockAccess, mockReadFile, mockRealpath } = vi.hoisted(() => ({
  mockAccess: vi.fn(),
  mockReadFile: vi.fn(),
  mockRealpath: vi.fn(),
}));

vi.mock("fs/promises", () => {
  const mockedModule = {
    access: mockAccess,
    readFile: mockReadFile,
    realpath: mockRealpath,
  };
  return { ...mockedModule, default: mockedModule };
});

const GEMINI_BINARY = "/usr/local/bin/gemini";
const OAUTH2_BUNDLE = path.join(
  "/usr/local",
  "node_modules",
  "@google",
  "gemini-cli-core",
  "dist",
  "src",
  "code_assist",
  "oauth2.js",
);

/** 실제 설치본에서 이 경로만 존재하는 상황을 흉내 낸다 */
function stubExistingPaths(existingPaths: string[]) {
  mockAccess.mockImplementation(async (targetPath: string) => {
    if (existingPaths.includes(targetPath)) {
      return undefined;
    }
    throw new Error(`ENOENT: ${targetPath}`);
  });
}

describe("resolveGeminiOAuthClient", () => {
  beforeEach(() => {
    vi.stubEnv("PATH", "/usr/local/bin:/usr/bin");
    mockRealpath.mockImplementation(async (targetPath: string) => targetPath);
  });

  afterEach(() => {
    vi.unstubAllEnvs();
    vi.resetAllMocks();
  });

  it("gemini 바이너리를 어디서도 찾지 못하면 null을 돌려준다", async () => {
    stubExistingPaths([]);

    await expect(resolveGeminiOAuthClient()).resolves.toBeNull();
    expect(mockReadFile).not.toHaveBeenCalled();
  });

  it("설치본 번들에서 OAuth client id와 secret을 추출한다", async () => {
    stubExistingPaths([GEMINI_BINARY, OAUTH2_BUNDLE]);
    mockReadFile.mockResolvedValue(
      `const OAUTH_CLIENT_ID = "bundle-client-id.apps.googleusercontent.com";\n`
        + `const OAUTH_CLIENT_SECRET = 'bundle-client-secret';\n`,
    );

    await expect(resolveGeminiOAuthClient()).resolves.toEqual({
      clientId: "bundle-client-id.apps.googleusercontent.com",
      clientSecret: "bundle-client-secret",
    });
  });

  it("번들 형식이 바뀌어 자격을 못 읽으면 null을 돌려준다", async () => {
    stubExistingPaths([GEMINI_BINARY, OAUTH2_BUNDLE]);
    mockReadFile.mockResolvedValue("export const clientConfig = loadFromVault();");

    await expect(resolveGeminiOAuthClient()).resolves.toBeNull();
  });
});
