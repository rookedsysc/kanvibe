/**
 * Next.js 커스텀 서버 부트스트랩.
 * tsx를 통해 server.ts를 로드하기 전에 AsyncLocalStorage를 전역에 등록하고,
 * .env 파일을 수동 파싱하여 Next.js prepare() 이전에 환경변수를 사용할 수 있게 한다.
 */
const { readFileSync, existsSync } = require("node:fs");
const { resolve } = require("node:path");

const envPath = resolve(".env");
if (existsSync(envPath)) {
  for (const line of readFileSync(envPath, "utf-8").split("\n")) {
    const match = line.match(/^([^#=\s]+)\s*=\s*(.*)$/);
    if (match && !process.env[match[1]]) {
      process.env[match[1]] = match[2].trim().replace(/^(['"])(.*)\1$/, "$2");
    }
  }
}

/** PORT로부터 DB_PORT를 자동 계산하고 .env에 동기화한다 (PORT + 1) */
if (!process.env.DB_PORT) {
  process.env.DB_PORT = String(parseInt(process.env.PORT || "4885", 10) + 1);
}
if (existsSync(envPath)) {
  const envContent = readFileSync(envPath, "utf-8");
  if (!envContent.match(/^DB_PORT=/m)) {
    const { writeFileSync } = require("node:fs");
    writeFileSync(envPath, envContent.trimEnd() + "\nDB_PORT=" + process.env.DB_PORT + "\n");
  }
}

const { AsyncLocalStorage } = require("node:async_hooks");
globalThis.AsyncLocalStorage = AsyncLocalStorage;
require("tsx/cjs");
require("./server.ts");
