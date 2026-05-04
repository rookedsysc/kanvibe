import { beforeEach, describe, expect, it, vi } from "vitest";
import { extractRegisteredKanvibePluginUrls, extractRegisteredPluginUrls, isKanvibePluginUrl } from "../openCodePluginRegistry";

const mocks = vi.hoisted(() => ({
  execFile: vi.fn(),
}));

vi.mock("node:child_process", async (importOriginal) => {
  const actual = await importOriginal<typeof import("node:child_process")>();
  return {
    ...actual,
    default: {
      ...actual,
      execFile: (...args: unknown[]) => mocks.execFile(...args),
    },
    execFile: (...args: unknown[]) => mocks.execFile(...args),
  };
});

describe("openCodePluginRegistry", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("extracts plugin urls from valid JSON output", () => {
    const output = JSON.stringify({
      plugin: [
        "file:///tmp/alpha.ts",
        "file:///tmp/kanvibe-plugin.ts",
      ],
      agent: {},
    });

    expect(extractRegisteredPluginUrls(output)).toEqual([
      "file:///tmp/alpha.ts",
      "file:///tmp/kanvibe-plugin.ts",
    ]);
  });

  it("falls back to the plugin block when opencode debug output is not valid JSON", () => {
    const output = `{
  "plugin": [
    "file:///tmp/alpha.ts",
    "file:///tmp/kanvibe-plugin.ts"
  ],
  "agent": {
    "custom": {
      "template": "line 1
line 2"
    }
  }
}`;

    expect(extractRegisteredPluginUrls(output)).toEqual([
      "file:///tmp/alpha.ts",
      "file:///tmp/kanvibe-plugin.ts",
    ]);
  });

  it("filters only kanvibe plugin urls", () => {
    const output = JSON.stringify({
      plugin: [
        "file:///tmp/alpha.ts",
        "file:///tmp/kanvibe-plugin.ts",
        "file:///tmp/kanvibe-plugin.js",
      ],
      agent: {},
    });

    expect(extractRegisteredKanvibePluginUrls(output)).toEqual([
      "file:///tmp/kanvibe-plugin.ts",
      "file:///tmp/kanvibe-plugin.js",
    ]);
  });

  it("matches kanvibe plugin urls with ts or js extensions", () => {
    expect(isKanvibePluginUrl("file:///tmp/kanvibe-plugin.ts")).toBe(true);
    expect(isKanvibePluginUrl("file:///tmp/kanvibe-plugin.js")).toBe(true);
    expect(isKanvibePluginUrl("file:///tmp/not-kanvibe.ts")).toBe(false);
  });

  it("runs opencode debug config with the local shell environment", async () => {
    const originalHome = process.env.HOME;
    process.env.HOME = "/home/opencode-user";
    mocks.execFile.mockImplementation((
      _file: string,
      _args: string[],
      _options: unknown,
      callback: (error: null, result: { stdout: string }) => void,
    ) => {
      callback(null, {
        stdout: JSON.stringify({
          plugin: ["file:///tmp/kanvibe-plugin.ts"],
        }),
      });
    });

    try {
      const { getOpenCodeRegisteredKanvibePluginUrls } = await import("../openCodePluginRegistry");

      const result = await getOpenCodeRegisteredKanvibePluginUrls("/workspace/project");

      expect(result).toEqual(["file:///tmp/kanvibe-plugin.ts"]);
      expect(mocks.execFile).toHaveBeenCalledWith(
        "opencode",
        ["debug", "config"],
        expect.objectContaining({
          cwd: "/workspace/project",
          env: expect.objectContaining({
            PATH: process.platform === "darwin"
              ? expect.stringContaining("/home/opencode-user/.opencode/bin")
              : expect.any(String),
          }),
        }),
        expect.any(Function),
      );
    } finally {
      if (originalHome === undefined) {
        delete process.env.HOME;
      } else {
        process.env.HOME = originalHome;
      }
    }
  });
});
