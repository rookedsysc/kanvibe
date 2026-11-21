import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { createRequire } from "node:module";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ensureSqliteDatabaseReady } from "../src/lib/sqliteSchema";

const require = createRequire(import.meta.url);
const Database = require("better-sqlite3");
const { prepareDevelopmentDatabase } = require("./developmentDatabase");

let directory;
let sourceDirectory;
let targetDirectory;
const databases = [];

function openDatabase(filename) {
  const database = new Database(filename);
  databases.push(database);
  return database;
}

function createApp({ switches = {}, target = targetDirectory, isPackaged = false, lock = true } = {}) {
  return {
    isPackaged,
    commandLine: {
      hasSwitch: (key) => Object.hasOwn(switches, key),
      getSwitchValue: (key) => switches[key] || "",
    },
    getPath: (key) => key === "appData" ? directory : target,
    requestSingleInstanceLock: vi.fn(() => lock),
  };
}

function prepare(app = createApp(), env = {}) {
  return prepareDevelopmentDatabase(app, { isDevelopment: true, env });
}

function sourceDatabase() {
  const filename = path.join(sourceDirectory, "kanvibe.db");
  ensureSqliteDatabaseReady(filename);
  const database = openDatabase(filename);
  database.pragma("journal_mode = WAL");
  database.pragma("wal_autocheckpoint = 0");
  database.exec(`
    INSERT INTO projects (id, name, repo_path) VALUES ('project', 'Application project', '/repo');
    INSERT INTO kanban_tasks (id, title, project_id) VALUES ('task', 'Application task', 'project');
    INSERT INTO app_settings (id, key, value) VALUES ('setting', 'theme', 'dark');
    CREATE TABLE additional_data (value TEXT);
    INSERT INTO additional_data VALUES ('included without a table allowlist');
  `);
  return database;
}

beforeEach(() => {
  directory = fs.mkdtempSync(path.join(os.tmpdir(), "kanvibe-copy-"));
  sourceDirectory = path.join(directory, "KanVibe");
  targetDirectory = path.join(directory, "kanvibe-dev");
  fs.mkdirSync(sourceDirectory);
  fs.mkdirSync(targetDirectory);
  vi.spyOn(console, "log").mockImplementation(() => {});
  vi.spyOn(console, "warn").mockImplementation(() => {});
});

afterEach(() => {
  for (const database of databases.splice(0)) if (database.open) database.close();
  vi.restoreAllMocks();
  fs.rmSync(directory, { recursive: true, force: true });
});

describe("development application dataset", () => {
  it("copies every table including committed WAL data and keeps edits separate", async () => {
    const source = sourceDatabase();
    const sourcePath = path.join(sourceDirectory, "kanvibe.db");
    const beforeDatabase = fs.readFileSync(sourcePath);
    const beforeWal = fs.readFileSync(`${sourcePath}-wal`);
    expect(beforeWal.length).toBeGreaterThan(0);

    await prepare(createApp({ switches: { "data-source": "application" } }));

    const target = openDatabase(path.join(targetDirectory, "kanvibe.db"));
    const tables = source.prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name").all();
    expect(target.prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name").all()).toEqual(tables);
    for (const { name } of tables) {
      const quoted = `"${name.replaceAll('"', '""')}"`;
      expect(target.prepare(`SELECT * FROM ${quoted}`).all()).toEqual(source.prepare(`SELECT * FROM ${quoted}`).all());
    }
    expect(target.pragma("integrity_check", { simple: true })).toBe("ok");
    target.exec("UPDATE kanban_tasks SET title = 'Development edit'; DELETE FROM app_settings");
    expect(source.prepare("SELECT title FROM kanban_tasks WHERE id = 'task'").get().title).toBe("Application task");
    expect(source.prepare("SELECT value FROM app_settings WHERE id = 'setting'").get().value).toBe("dark");
    expect(fs.readFileSync(sourcePath)).toEqual(beforeDatabase);
    expect(fs.readFileSync(`${sourcePath}-wal`)).toEqual(beforeWal);
  });

  it("refreshes an existing development database when application mode is requested", async () => {
    const source = sourceDatabase();
    await prepare(createApp({ switches: { "data-source": "application" } }));
    const target = openDatabase(path.join(targetDirectory, "kanvibe.db"));
    target.exec("UPDATE kanban_tasks SET title = 'Previous development edit'");
    target.close();
    source.exec("UPDATE kanban_tasks SET title = 'Latest application task'");

    await prepare(createApp({ switches: { "data-source": "application" } }));

    expect(openDatabase(path.join(targetDirectory, "kanvibe.db"))
      .prepare("SELECT title FROM kanban_tasks WHERE id = 'task'").get().title).toBe("Latest application task");
  });

  it("does not copy the application's hook client ID into the development database", async () => {
    const source = sourceDatabase();
    source.exec("INSERT INTO app_settings (id, key, value) VALUES ('client', 'hook_client_id', 'application-client')");

    await prepare(createApp({ switches: { "data-source": "application" } }));

    const target = openDatabase(path.join(targetDirectory, "kanvibe.db"));
    expect(target.prepare("SELECT value FROM app_settings WHERE key = 'hook_client_id'").get()).toBeUndefined();
    expect(target.prepare("SELECT value FROM app_settings WHERE key = 'theme'").get().value).toBe("dark");
    expect(source.prepare("SELECT value FROM app_settings WHERE key = 'hook_client_id'").get().value).toBe("application-client");
  });

  it("keeps the development hook client ID across database refreshes", async () => {
    const source = sourceDatabase();
    source.exec("INSERT INTO app_settings (id, key, value) VALUES ('client', 'hook_client_id', 'application-client')");
    await prepare(createApp({ switches: { "data-source": "application" } }));
    const targetPath = path.join(targetDirectory, "kanvibe.db");
    const previousTarget = openDatabase(targetPath);
    previousTarget.exec("INSERT INTO app_settings (id, key, value) VALUES ('dev-client', 'hook_client_id', 'development-client')");
    previousTarget.close();

    await prepare(createApp({ switches: { "data-source": "application" } }));

    expect(openDatabase(targetPath).prepare("SELECT value FROM app_settings WHERE key = 'hook_client_id'").all())
      .toEqual([{ value: "development-client" }]);
  });

  it("drops a development hook client ID that an older copy took from the application", async () => {
    const source = sourceDatabase();
    source.exec("INSERT INTO app_settings (id, key, value) VALUES ('client', 'hook_client_id', 'application-client')");
    const targetPath = path.join(targetDirectory, "kanvibe.db");
    // 이 수정 전의 복사본처럼 설치 앱의 ID가 그대로 들어 있는 dev DB를 만든다.
    await source.backup(targetPath);

    await prepare(createApp({ switches: { "data-source": "application" } }));

    expect(openDatabase(targetPath).prepare("SELECT value FROM app_settings WHERE key = 'hook_client_id'").get())
      .toBeUndefined();
  });

  it.each(["", "existing"])("preserves development edits across restarts in %j mode", async (mode) => {
    sourceDatabase();
    const target = openDatabase(path.join(targetDirectory, "kanvibe.db"));
    target.exec("CREATE TABLE dev_only (value TEXT); INSERT INTO dev_only VALUES ('keep')");

    target.close();
    const switches = mode ? { "data-source": mode } : {};

    await prepare(createApp({ switches }));
    await prepare(createApp({ switches }));

    expect(openDatabase(path.join(targetDirectory, "kanvibe.db"))
      .prepare("SELECT value FROM dev_only").all()).toEqual([{ value: "keep" }]);
  });

  it.each([false, true])("does not import application data into a new development database by default (custom source: %s)", async (hasCustomSource) => {
    sourceDatabase();
    const switches = hasCustomSource ? { "application-data-dir": sourceDirectory } : {};

    await prepare(createApp({ switches }));

    expect(fs.existsSync(path.join(targetDirectory, "kanvibe.db"))).toBe(false);
  });

  it("keeps development data by default even when the application database is corrupt", async () => {
    fs.writeFileSync(path.join(sourceDirectory, "kanvibe.db"), "corrupt source");
    const target = openDatabase(path.join(targetDirectory, "kanvibe.db"));
    target.exec("CREATE TABLE retained (value TEXT); INSERT INTO retained VALUES ('keep')");
    target.close();

    await prepare();

    expect(openDatabase(path.join(targetDirectory, "kanvibe.db"))
      .prepare("SELECT value FROM retained").all()).toEqual([{ value: "keep" }]);
  });

  it("keeps an explicit app-data fixture by default", async () => {
    sourceDatabase();
    await prepare(createApp(), { KANVIBE_APP_DATA_DIR: targetDirectory });
    expect(fs.existsSync(path.join(targetDirectory, "kanvibe.db"))).toBe(false);
  });

  it("copies from an explicitly selected application directory into an explicit target", async () => {
    sourceDatabase().close();
    const customSource = path.join(directory, "custom-installation");
    fs.renameSync(sourceDirectory, customSource);
    await prepare(createApp({ switches: { "data-source": "application", "application-data-dir": customSource } }),
      { KANVIBE_APP_DATA_DIR: targetDirectory });
    expect(openDatabase(path.join(targetDirectory, "kanvibe.db"))
      .prepare("SELECT value FROM additional_data").get().value).toContain("included");
  });

  it.each([
    [{ isPackaged: true }, true, {}],
    [{}, false, {}],
    [{}, true, { KANVIBE_QA_MODE: "1" }],
    [{ switches: { qa: true } }, true, {}],
  ])("leaves packaged/start/automated/manual QA data alone (%j)", async (options, isDevelopment, env) => {
    sourceDatabase();
    const app = createApp(options);
    await prepareDevelopmentDatabase(app, { isDevelopment, env });
    expect(app.requestSingleInstanceLock).not.toHaveBeenCalled();
    expect(fs.existsSync(path.join(targetDirectory, "kanvibe.db"))).toBe(false);
  });

  it("warns and retains development data when no application database is installed", async () => {
    const targetPath = path.join(targetDirectory, "kanvibe.db");
    fs.writeFileSync(targetPath, "existing fixture");
    await prepare(createApp({ switches: { "data-source": "application" } }));
    expect(console.warn).toHaveBeenCalledWith(expect.stringContaining("Application database not found"));
    expect(fs.readFileSync(targetPath, "utf8")).toBe("existing fixture");
  });

  it("rejects a missing explicitly requested source", async () => {
    await expect(prepare(createApp({ switches: { "data-source": "application", "application-data-dir": path.join(directory, "missing") } })))
      .rejects.toThrow("Application database not found");
    expect(fs.existsSync(path.join(targetDirectory, "kanvibe.db"))).toBe(false);
  });

  it("preserves the old development database if the application database is corrupt", async () => {
    fs.writeFileSync(path.join(sourceDirectory, "kanvibe.db"), "corrupt source");
    const target = openDatabase(path.join(targetDirectory, "kanvibe.db"));
    target.exec("CREATE TABLE retained (value TEXT); INSERT INTO retained VALUES ('keep')");
    await expect(prepare(createApp({ switches: { "data-source": "application" } }))).rejects.toThrow();
    expect(target.prepare("SELECT value FROM retained").get().value).toBe("keep");
  });

  it.each(["application", "existing"])("rejects the application directory even in %s mode", async (mode) => {
    await expect(prepare(createApp({ target: sourceDirectory, switches: { "data-source": mode } })))
      .rejects.toThrow("separate database directories");
    expect(fs.existsSync(path.join(sourceDirectory, "kanvibe.db"))).toBe(false);
  });

  it("rejects a development directory symlink to application data", async () => {
    sourceDatabase();
    const alias = path.join(directory, "alias");
    fs.symlinkSync(sourceDirectory, alias, "dir");
    await expect(prepare(createApp({ target: alias }))).rejects.toThrow("separate database directories");
  });

  it.each(["symlink", "hardlink"])("rejects a development DB %s to the application database", async (kind) => {
    sourceDatabase();
    const sourcePath = path.join(sourceDirectory, "kanvibe.db");
    const targetPath = path.join(targetDirectory, "kanvibe.db");
    if (kind === "symlink") fs.symlinkSync(sourcePath, targetPath);
    else fs.linkSync(sourcePath, targetPath);
    await expect(prepare()).rejects.toThrow("separate database files");
  });

  it("rejects a dangling development DB symlink before it can create a shared database", async () => {
    fs.symlinkSync(path.join(sourceDirectory, "kanvibe.db"), path.join(targetDirectory, "kanvibe.db"));
    await expect(prepare()).rejects.toThrow("separate database files");
    expect(fs.existsSync(path.join(sourceDirectory, "kanvibe.db"))).toBe(false);
  });

  it("rejects a second development instance before refreshing its database", async () => {
    sourceDatabase();
    await expect(prepare(createApp({ lock: false }))).rejects.toThrow("Another development app");
    expect(fs.existsSync(path.join(targetDirectory, "kanvibe.db"))).toBe(false);
  });

  it("rejects an unknown data-source mode", async () => {
    await expect(prepare(createApp({ switches: { "data-source": "typo" } }))).rejects.toThrow("--data-source");
  });
});
