#!/usr/bin/env node

try {
  const BetterSqlite3 = require("better-sqlite3");

  const database = new BetterSqlite3(":memory:");

  try {
    database.pragma("journal_mode = WAL");
    database.prepare("SELECT 1").get();
  } finally {
    database.close();
  }

  process.exit(0);
} catch (error) {
  console.error(String(error?.stack || error?.message || error));
  process.exit(1);
}
