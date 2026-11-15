const { randomUUID } = require("node:crypto");
const fs = require("node:fs");
const path = require("node:path");

function assertSeparateDatabase(sourceDirectory, targetDirectory) {
  fs.mkdirSync(targetDirectory, { recursive: true });
  const sourceRealDirectory = fs.existsSync(sourceDirectory) ? fs.realpathSync(sourceDirectory) : path.resolve(sourceDirectory);
  if (sourceRealDirectory === fs.realpathSync(targetDirectory)) {
    throw new Error("Development and application must use separate database directories");
  }

  const sourcePath = path.join(sourceDirectory, "kanvibe.db");
  const targetPath = path.join(targetDirectory, "kanvibe.db");
  const target = fs.lstatSync(targetPath, { throwIfNoEntry: false });
  if (target) {
    const source = fs.existsSync(sourcePath) ? fs.statSync(sourcePath) : null;
    if (target.isSymbolicLink() || (source && source.dev === target.dev && source.ino === target.ino)) {
      throw new Error("Development and application must use separate database files without links");
    }
  }
}

async function prepareDevelopmentDatabase(app, { isDevelopment, env = process.env }) {
  if (!isDevelopment || app.isPackaged || env.KANVIBE_QA_MODE === "1" || app.commandLine.hasSwitch("qa")) {
    return;
  }

  const requestedMode = app.commandLine.getSwitchValue("data-source");
  const mode = requestedMode || "existing";
  if (mode !== "application" && mode !== "existing") {
    throw new Error("--data-source must be application or existing");
  }

  const requestedSource = app.commandLine.getSwitchValue("application-data-dir");
  const sourceDirectory = path.resolve(requestedSource || path.join(app.getPath("appData"), "KanVibe"));
  const targetDirectory = app.getPath("userData");
  const sourcePath = path.join(sourceDirectory, "kanvibe.db");

  assertSeparateDatabase(sourceDirectory, targetDirectory);

  if (!app.requestSingleInstanceLock()) {
    throw new Error("Another development app is using this data directory. Close it before restarting.");
  }

  if (mode === "existing") return;
  if (!fs.existsSync(sourcePath)) {
    if (requestedSource) throw new Error(`Application database not found: ${sourcePath}`);
    console.warn(`[kanvibe] Application database not found at ${sourcePath}; using the existing development database or bundled seed.`);
    return;
  }

  const Database = require("better-sqlite3");
  const source = new Database(sourcePath, { readonly: true, fileMustExist: true });
  const targetPath = path.join(targetDirectory, "kanvibe.db");
  try {
    const applicationHookClientId = readHookClientId(source);
    const developmentHookClientId = readPreviousDevelopmentHookClientId(Database, targetPath);
    // Online backup includes committed WAL pages while the installed app stays open.
    await source.backup(targetPath);
    replaceHookClientId(
      Database,
      targetPath,
      developmentHookClientId === applicationHookClientId ? null : developmentHookClientId,
    );
    console.log(`[kanvibe] Copied application database: ${sourcePath} -> ${targetPath}`);
  } finally {
    source.close();
  }
}

/**
 * hook client ID는 targets.json 항목과 원격 터널 소켓 이름을 정하는 설치별 ID다.
 * 설치 앱의 ID를 dev 앱이 함께 쓰면 dev 앱이 targets.json의 설치 앱 항목을 덮어써 설치 앱에 알림이 오지 않으므로,
 * 복사본에는 dev 앱이 이전에 쓰던 ID만 남기고 없으면 dev 앱이 처음 쓸 때 새로 만들게 둔다.
 */
const HOOK_CLIENT_ID_KEY = "hook_client_id";

function readHookClientId(database) {
  return database.prepare("SELECT value FROM app_settings WHERE key = ?").get(HOOK_CLIENT_ID_KEY)?.value ?? null;
}

function readPreviousDevelopmentHookClientId(Database, targetPath) {
  if (!fs.existsSync(targetPath)) return null;
  let target;
  try {
    target = new Database(targetPath, { readonly: true, fileMustExist: true });
    return readHookClientId(target);
  } catch {
    // 읽을 수 없는 이전 dev DB는 곧 덮어쓰므로 dev 앱이 새 ID를 받게 둔다.
    return null;
  } finally {
    target?.close();
  }
}

function replaceHookClientId(Database, targetPath, hookClientId) {
  const target = new Database(targetPath, { fileMustExist: true });
  try {
    target.prepare("DELETE FROM app_settings WHERE key = ?").run(HOOK_CLIENT_ID_KEY);
    if (hookClientId) {
      target.prepare("INSERT INTO app_settings (id, key, value) VALUES (?, ?, ?)")
        .run(randomUUID(), HOOK_CLIENT_ID_KEY, hookClientId);
    }
  } finally {
    target.close();
  }
}

module.exports = { prepareDevelopmentDatabase };
