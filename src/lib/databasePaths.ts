import fs from "node:fs";
import os from "node:os";
import path from "node:path";

function getDefaultDataDirectory(): string {
  if (process.env.KANVIBE_APP_DATA_DIR) {
    return process.env.KANVIBE_APP_DATA_DIR;
  }

  if (process.env.KANVIBE_DB_PATH) {
    return path.dirname(process.env.KANVIBE_DB_PATH);
  }

  if (process.env.NODE_ENV === "test") {
    return path.join(os.tmpdir(), `kanvibe-test-${process.pid}`);
  }

  return path.join(process.cwd(), ".kanvibe");
}

export function ensureKanvibeDataDirectory(): string {
  const dataDirectory = getDefaultDataDirectory();
  fs.mkdirSync(dataDirectory, { recursive: true });
  return dataDirectory;
}

export function getRuntimeDatabasePath(): string {
  return process.env.KANVIBE_DB_PATH ?? path.join(ensureKanvibeDataDirectory(), "kanvibe.db");
}

export function getBundledSeedDatabasePath(): string {
  return process.env.KANVIBE_SEED_DB_PATH ?? path.join(process.cwd(), "resources", "database", "app.seed.db");
}

export function ensureRuntimeDatabaseFile(): string {
  const databasePath = getRuntimeDatabasePath();
  if (fs.existsSync(databasePath)) {
    return databasePath;
  }

  fs.mkdirSync(path.dirname(databasePath), { recursive: true });

  const bundledSeedPath = getBundledSeedDatabasePath();
  if (fs.existsSync(bundledSeedPath)) {
    fs.copyFileSync(bundledSeedPath, databasePath);
  } else {
    fs.closeSync(fs.openSync(databasePath, "a"));
  }

  return databasePath;
}
