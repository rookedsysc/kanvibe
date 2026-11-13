import { createRequire } from "node:module";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import vm from "node:vm";
import { afterEach, describe, expect, it, vi } from "vitest";

const require = createRequire(import.meta.url);
const fixtureDirectories = [];

afterEach(() => {
  vi.unstubAllEnvs();
  for (const directory of fixtureDirectories.splice(0)) {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});

function createLauncher() {
  const page = { setDefaultTimeout() {}, setDefaultNavigationTimeout() {}, async setViewportSize() {} };
  const launch = vi.fn(async () => ({
    process: () => ({}), firstWindow: async () => page, close: async () => {},
  }));
  const module = { exports: {} };
  vm.runInNewContext(fs.readFileSync(new URL("./launchElectron.cjs", import.meta.url), "utf8"), {
    module, process,
    require: (name) => {
      if (name === "@playwright/test") return { _electron: { launch } };
      if (name === "electron") return "/fixture/electron";
      return require(name);
    },
  });
  return { launch, launchKanVibeElectron: module.exports.launchKanVibeElectron };
}

describe("Electron QA launcher", () => {
  it("honors an explicit CDP port when provided", async () => {
    const { resolveCdpPort } = require("./launchElectron.cjs");

    await expect(resolveCdpPort({ env: { KANVIBE_QA_CDP_PORT: "19444" } })).resolves.toBe(19444);
  });

  it("allocates a dynamic CDP port when no port is configured so parallel QA runs do not collide", async () => {
    const { resolveCdpPort } = require("./launchElectron.cjs");

    const port = await resolveCdpPort({ env: {} });

    expect(Number.isInteger(port)).toBe(true);
    expect(port).toBeGreaterThan(0);
    expect(port).not.toBe(19337);
  });

  it("uses an explicit QA fixture for both app data and database despite inherited production paths", async () => {
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), "kanvibe-launch-test-"));
    fixtureDirectories.push(directory);
    vi.stubEnv("KANVIBE_APP_DATA_DIR", "/production/app-data");
    vi.stubEnv("KANVIBE_DB_PATH", "/production/kanvibe.db");
    const { launch, launchKanVibeElectron } = createLauncher();

    await launchKanVibeElectron({ appDataDir: directory });

    expect(launch.mock.calls[0][0].env).toMatchObject({
      KANVIBE_APP_DATA_DIR: directory,
      KANVIBE_DB_PATH: path.join(directory, "kanvibe.db"),
      KANVIBE_QA_MODE: "1",
    });
    expect(process.env.KANVIBE_DB_PATH).toBe("/production/kanvibe.db");
  });

  it("allocates separate temporary databases when a QA caller omits appDataDir", async () => {
    vi.stubEnv("KANVIBE_APP_DATA_DIR", "/production/app-data");
    vi.stubEnv("KANVIBE_DB_PATH", "/production/kanvibe.db");
    const { launch, launchKanVibeElectron } = createLauncher();

    await launchKanVibeElectron();
    await launchKanVibeElectron();
    const first = launch.mock.calls[0][0].env;
    const second = launch.mock.calls[1][0].env;
    for (const env of [first, second]) {
      if (env.KANVIBE_APP_DATA_DIR.startsWith(os.tmpdir())) fixtureDirectories.push(env.KANVIBE_APP_DATA_DIR);
      expect(env.KANVIBE_APP_DATA_DIR).toContain(path.join(os.tmpdir(), "kanvibe-electron-qa-"));
      expect(fs.existsSync(env.KANVIBE_APP_DATA_DIR)).toBe(true);
      expect(env.KANVIBE_DB_PATH).toBe(path.join(env.KANVIBE_APP_DATA_DIR, "kanvibe.db"));
    }
    expect(first.KANVIBE_APP_DATA_DIR).not.toBe(second.KANVIBE_APP_DATA_DIR);
  });

  it("gives each automated QA run a hook port apart from the installed app and pnpm dev", async () => {
    const { launch, launchKanVibeElectron } = createLauncher();

    const first = await launchKanVibeElectron();
    const second = await launchKanVibeElectron();
    for (const call of launch.mock.calls) fixtureDirectories.push(call[0].env.KANVIBE_APP_DATA_DIR);

    expect(launch.mock.calls[0][0].args).toContain(`--kanvibe-hook-port=${first.hookPort}`);
    expect(launch.mock.calls[1][0].args).toContain(`--kanvibe-hook-port=${second.hookPort}`);
    expect(first.hookPort).not.toBe(second.hookPort);
    for (const hookPort of [first.hookPort, second.hookPort]) {
      expect([9736, 19736, 29736]).not.toContain(hookPort);
      expect(hookPort + 10).toBeLessThanOrEqual(65535);
    }
  });

  it("uses the fixed hook port a mobile QA fixture asks for", async () => {
    const { launch, launchKanVibeElectron } = createLauncher();

    const launched = await launchKanVibeElectron({ hookPort: 39736 });
    fixtureDirectories.push(launch.mock.calls[0][0].env.KANVIBE_APP_DATA_DIR);

    expect(launched.hookPort).toBe(39736);
    expect(launch.mock.calls[0][0].args).toContain("--kanvibe-hook-port=39736");
  });
});
