import { createRequire } from "node:module";
import path from "node:path";
import { describe, expect, it, vi } from "vitest";

const require = createRequire(import.meta.url);
const { applyAppDataDirectoryOverride } = require("./runtimeEnvironment");

function createApp({ isPackaged = false, qa = false } = {}) {
  const paths = { appData: "/home/user/.config", userData: "/home/user/.config/kanvibe" };
  return {
    isPackaged,
    commandLine: { hasSwitch: (name) => name === "qa" && qa },
    getPath: vi.fn((name) => paths[name]),
    setPath: vi.fn((name, value) => { paths[name] = value; }),
  };
}

describe("Electron runtime environment", () => {
  it("uses KANVIBE_APP_DATA_DIR as Electron userData when it is provided", () => {
    const app = {
      getPath: vi.fn(() => "/home/user/.config/kanvibe"),
      setPath: vi.fn(),
    };

    const { applyAppDataDirectoryOverride } = require("./runtimeEnvironment");

    const userDataPath = applyAppDataDirectoryOverride(app, {
      KANVIBE_APP_DATA_DIR: "/tmp/kanvibe-qa-run/app-data",
    });

    expect(app.setPath).toHaveBeenCalledWith("userData", "/tmp/kanvibe-qa-run/app-data");
    expect(userDataPath).toBe("/tmp/kanvibe-qa-run/app-data");
  });

  it("keeps the installed production app's existing userData path", () => {
    const app = createApp({ isPackaged: true });

    const { applyAppDataDirectoryOverride } = require("./runtimeEnvironment");

    const userDataPath = applyAppDataDirectoryOverride(app, {});

    expect(app.setPath).not.toHaveBeenCalled();
    expect(userDataPath).toBe("/home/user/.config/kanvibe");
  });

  it.each([
    [{ KANVIBE_RENDERER_URL: "http://127.0.0.1:5173" }, {}, "kanvibe-dev"],
    [{ KANVIBE_RENDERER_URL: "http://127.0.0.1:5199" }, {}, "kanvibe-dev"],
    [{}, {}, "kanvibe-qa"],
    [{}, { isPackaged: true, qa: true }, "kanvibe-qa"],
    [{ KANVIBE_RENDERER_URL: "http://127.0.0.1:5173" }, { qa: true }, "kanvibe-qa"],
  ])("selects persistent data for env=%j app=%j", (env, options, directory) => {
    const app = createApp(options);
    const expectedPath = path.join("/home/user/.config", directory);

    expect(applyAppDataDirectoryOverride(app, env)).toBe(expectedPath);
    expect(app.getPath("userData")).toBe(expectedPath);
  });

  it("keeps manual QA on the same directory when started again", () => {
    const firstApp = createApp();
    const restartedApp = createApp();

    expect(applyAppDataDirectoryOverride(firstApp, {})).toBe(
      applyAppDataDirectoryOverride(restartedApp, {}),
    );
  });

  it("treats a blank data directory override as absent", () => {
    expect(applyAppDataDirectoryOverride(createApp(), { KANVIBE_APP_DATA_DIR: "  " }))
      .toBe(path.join("/home/user/.config", "kanvibe-qa"));
  });

  it("refuses automated QA without a fixture directory", () => {
    expect(() => applyAppDataDirectoryOverride(createApp(), { KANVIBE_QA_MODE: "1" }))
      .toThrow(/KANVIBE_APP_DATA_DIR/);
  });

  it("uses the requested fixture directory for automated QA", () => {
    expect(applyAppDataDirectoryOverride(createApp(), {
      KANVIBE_QA_MODE: "1", KANVIBE_APP_DATA_DIR: " /tmp/kanvibe-fixture ",
    })).toBe("/tmp/kanvibe-fixture");
  });
});
