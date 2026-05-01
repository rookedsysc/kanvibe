import os from "node:os";
import path from "node:path";
import { defineConfig, devices } from "@playwright/test";

const appDataDir = path.join(os.tmpdir(), `kanvibe-playwright-appdata-${process.pid}`);
const nodeBinDir = path.dirname(process.execPath);
const runtimePath = `${nodeBinDir}:${process.env.PATH ?? ""}`;

export default defineConfig({
  testDir: "./tests/e2e",
  timeout: 60_000,
  fullyParallel: false,
  retries: 0,
  reporter: "list",
  use: {
    ...devices["Desktop Chrome"],
    baseURL: "http://127.0.0.1:4885",
    headless: true,
  },
  webServer: {
    command: `bash -lc 'for port in 4884 4885; do fuser -k "$port"/tcp 2>/dev/null || true; done; rm -f .next/dev/lock; rm -rf "${appDataDir}"; pnpm db:prepare && pnpm dev'`,
    url: "http://127.0.0.1:4885/ko/login",
    timeout: 180_000,
    reuseExistingServer: false,
    env: {
      ...process.env,
      PATH: runtimePath,
      KANVIBE_APP_DATA_DIR: appDataDir,
    },
  },
});
