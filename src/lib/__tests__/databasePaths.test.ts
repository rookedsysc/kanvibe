import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ensureRuntimeDatabaseFile, getRuntimeDatabasePath } from "@/lib/databasePaths";

it("runs unit tests with an explicit isolated database instead of inherited app paths", () => {
  const directory = process.env.KANVIBE_APP_DATA_DIR;
  expect(directory).toBeTruthy();
  expect(directory).toContain(path.join(os.tmpdir(), "kanvibe-vitest-"));
  expect(process.env.KANVIBE_DB_PATH).toBe(path.join(directory!, "kanvibe.db"));
  expect(process.env.KANVIBE_DESKTOP).not.toBe("true");
});

describe("runtime database paths", () => {
  let fixtureDirectory: string;

  beforeEach(() => {
    fixtureDirectory = fs.mkdtempSync(path.join(os.tmpdir(), "kanvibe-paths-test-"));
    vi.stubEnv("KANVIBE_APP_DATA_DIR", path.join(fixtureDirectory, "qa"));
    vi.stubEnv("KANVIBE_DB_PATH", path.join(fixtureDirectory, "production", "kanvibe.db"));
    vi.stubEnv("KANVIBE_SEED_DB_PATH", path.join(fixtureDirectory, "app.seed.db"));
    vi.stubEnv("KANVIBE_DESKTOP", "true");
  });

  afterEach(() => {
    vi.unstubAllEnvs();
    fs.rmSync(fixtureDirectory, { recursive: true, force: true });
  });

  it("uses the desktop's selected userData even when a different DB path was inherited", () => {
    expect(getRuntimeDatabasePath()).toBe(path.join(fixtureDirectory, "qa", "kanvibe.db"));
    expect(fs.existsSync(path.join(fixtureDirectory, "production"))).toBe(false);
  });

  it("keeps explicit DB path overrides available to non-desktop tools", () => {
    vi.stubEnv("KANVIBE_DESKTOP", "");
    expect(getRuntimeDatabasePath()).toBe(path.join(fixtureDirectory, "production", "kanvibe.db"));
  });

  it("preserves QA database contents across startup even after the bundled seed changes", () => {
    const expectedDatabasePath = path.join(fixtureDirectory, "qa", "kanvibe.db");
    fs.mkdirSync(path.dirname(expectedDatabasePath), { recursive: true });
    fs.writeFileSync(expectedDatabasePath, "existing QA database fixture");
    fs.writeFileSync(path.join(fixtureDirectory, "app.seed.db"), "new version seed fixture");

    expect(ensureRuntimeDatabaseFile()).toBe(expectedDatabasePath);
    expect(ensureRuntimeDatabaseFile()).toBe(expectedDatabasePath);
    expect(fs.readFileSync(expectedDatabasePath, "utf8")).toBe("existing QA database fixture");
    expect(fs.existsSync(path.join(fixtureDirectory, "production"))).toBe(false);
  });

  it("seeds a new QA database only once", () => {
    fs.writeFileSync(path.join(fixtureDirectory, "app.seed.db"), "initial seed fixture");
    const databasePath = ensureRuntimeDatabaseFile();
    expect(fs.readFileSync(databasePath, "utf8")).toBe("initial seed fixture");

    fs.writeFileSync(path.join(fixtureDirectory, "app.seed.db"), "updated seed fixture");
    ensureRuntimeDatabaseFile();
    expect(fs.readFileSync(databasePath, "utf8")).toBe("initial seed fixture");
  });
});
