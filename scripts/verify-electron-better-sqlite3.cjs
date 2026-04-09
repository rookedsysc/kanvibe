#!/usr/bin/env node

const BetterSqlite3 = require("better-sqlite3");

const database = new BetterSqlite3(":memory:");

try {
  database.pragma("journal_mode = WAL");
  database.prepare("SELECT 1").get();
} finally {
  database.close();
}

process.exit(0);
