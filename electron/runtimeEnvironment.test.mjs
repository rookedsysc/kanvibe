import { createRequire } from "node:module";
import { describe, expect, it, vi } from "vitest";

const require = createRequire(import.meta.url);

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

  it("keeps Electron's default userData path when KANVIBE_APP_DATA_DIR is not provided", () => {
    const app = {
      getPath: vi.fn(() => "/home/user/.config/kanvibe"),
      setPath: vi.fn(),
    };

    const { applyAppDataDirectoryOverride } = require("./runtimeEnvironment");

    const userDataPath = applyAppDataDirectoryOverride(app, {});

    expect(app.setPath).not.toHaveBeenCalled();
    expect(userDataPath).toBe("/home/user/.config/kanvibe");
  });
});
