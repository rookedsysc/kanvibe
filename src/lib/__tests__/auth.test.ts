import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const cookieStore = {
  value: undefined as string | undefined,
  set: vi.fn((name: string, value: string) => {
    cookieStore.value = `${name}=${encodeURIComponent(value)}`;
  }),
  get: vi.fn(() => {
    if (!cookieStore.value) {
      return undefined;
    }

    const [, value = ""] = cookieStore.value.split("=");
    return { value: decodeURIComponent(value) };
  }),
  delete: vi.fn(() => {
    cookieStore.value = undefined;
  }),
};

const headerStore = new Headers();

vi.mock("next/headers", () => ({
  cookies: vi.fn(async () => cookieStore),
  headers: vi.fn(async () => headerStore),
}));

describe("auth session signing", () => {
  const originalEnv = { ...process.env };
  let appDataDir = "";

  beforeEach(() => {
    vi.resetModules();
    cookieStore.value = undefined;
    cookieStore.set.mockClear();
    cookieStore.get.mockClear();
    cookieStore.delete.mockClear();
    headerStore.delete("x-forwarded-proto");

    appDataDir = fs.mkdtempSync(path.join(os.tmpdir(), "kanvibe-auth-test-"));
    process.env = {
      ...originalEnv,
      KANVIBE_APP_DATA_DIR: appDataDir,
      KANVIBE_PASSWORD: "known-password",
    };
    delete process.env.KANVIBE_SESSION_SECRET;
  });

  afterEach(() => {
    fs.rmSync(appDataDir, { recursive: true, force: true });
    process.env = { ...originalEnv };
  });

  it("stores a separate persisted session secret instead of reusing the login password", async () => {
    const auth = await import("@/lib/auth");

    await auth.createSession();

    const secretPath = path.join(appDataDir, "session-secret");
    expect(fs.existsSync(secretPath)).toBe(true);
    expect(fs.readFileSync(secretPath, "utf8").trim()).not.toBe("known-password");
  });

  it("keeps existing signed cookies valid after the module reloads", async () => {
    const firstAuth = await import("@/lib/auth");
    await firstAuth.createSession();

    const cookieHeader = cookieStore.value;
    expect(cookieHeader).toBeTruthy();

    vi.resetModules();
    const reloadedAuth = await import("@/lib/auth");

    expect(reloadedAuth.validateSessionFromCookie(cookieHeader!)).toBe(true);
  });
});
