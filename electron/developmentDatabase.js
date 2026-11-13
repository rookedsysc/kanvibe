const fs = require("node:fs");
const path = require("node:path");
const { getRequestedAppDataDirectory } = require("./runtimeEnvironment");

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
  const mode = requestedMode || (getRequestedAppDataDirectory(env) ? "existing" : "application");
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
    // Online backup includes committed WAL pages while the installed app stays open.
    await source.backup(targetPath);
    console.log(`[kanvibe] Copied application database: ${sourcePath} -> ${targetPath}`);
  } finally {
    source.close();
  }
}

module.exports = { prepareDevelopmentDatabase };
