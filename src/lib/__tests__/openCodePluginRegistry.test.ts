import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
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
    vi.resetModules();
  });

  afterEach(() => vi.useRealTimers());

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


describe("OpenCode registration query lifecycle", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.resetModules();
    mocks.execFile.mockImplementation((_file, _args, _options, callback) => {
      callback(null, { stdout: '{"plugin":[]}' });
    });
  });

  afterEach(() => vi.useRealTimers());

  it("shares concurrent requests and caches sequential requests for five seconds", async () => {
    vi.useFakeTimers();
    const { getOpenCodeRegisteredKanvibePluginUrls: query } = await import("../openCodePluginRegistry");
    await Promise.all(Array.from({ length: 10 }, () => query("/same/repo")));
    await query("/same/repo/.");
    expect(mocks.execFile).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(5000);
    await query("/same/repo");
    expect(mocks.execFile).toHaveBeenCalledTimes(2);
    await query("/other/repo");
    expect(mocks.execFile).toHaveBeenCalledTimes(3);
  });

  it("bounds a slow CLI and retains failure instead of reporting an empty registration", async () => {
    vi.useFakeTimers();
    mocks.execFile.mockImplementation((_file, _args, options, callback) => {
      setTimeout(() => callback(new Error("OpenCode timed out")), options.timeout ?? 10000);
    });
    const { getOpenCodeRegisteredKanvibePluginUrls: query } = await import("../openCodePluginRegistry");
    const checks = Promise.all([
      expect(query("/slow/repo")).rejects.toThrow("OpenCode timed out"),
      expect(query("/slow/repo")).rejects.toThrow("OpenCode timed out"),
    ]);
    await vi.advanceTimersByTimeAsync(3000);
    await checks;
    expect(mocks.execFile).toHaveBeenCalledTimes(1);
    expect(mocks.execFile.mock.calls[0][2]).toMatchObject({ timeout: 3000, killSignal: "SIGKILL" });
    await expect(query("/slow/repo")).rejects.toThrow("OpenCode timed out");
    expect(mocks.execFile).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(5000);
    mocks.execFile.mockImplementation((_file, _args, _options, callback) => callback(null, { stdout: '{"plugin":[]}' }));
    await expect(query("/slow/repo")).resolves.toEqual([]);
    expect(mocks.execFile).toHaveBeenCalledTimes(2);
  });

  it("invalidates a completed query after installation", async () => {
    const registry = await import("../openCodePluginRegistry");
    await registry.getOpenCodeRegisteredKanvibePluginUrls("/installed/repo");
    registry.invalidateOpenCodePluginRegistration("/installed/repo");
    mocks.execFile.mockImplementation((_file, _args, _options, callback) => callback(null, {
      stdout: '{"plugin":["file:///installed/repo/.opencode/plugins/kanvibe-plugin.ts"]}',
    }));
    await expect(registry.getOpenCodeRegisteredKanvibePluginUrls("/installed/repo")).resolves.toHaveLength(1);
    expect(mocks.execFile).toHaveBeenCalledTimes(2);
  });

  it("waits for an invalidated in-flight query before sharing one fresh query", async () => {
    let finish!: (error: null, result: { stdout: string }) => void;
    mocks.execFile.mockImplementation((_file, _args, _options, callback) => { finish = callback; });
    const registry = await import("../openCodePluginRegistry");
    const oldQuery = registry.getOpenCodeRegisteredKanvibePluginUrls("/installing/repo");
    registry.invalidateOpenCodePluginRegistration("/installing/repo");
    const freshA = registry.getOpenCodeRegisteredKanvibePluginUrls("/installing/repo");
    const freshB = registry.getOpenCodeRegisteredKanvibePluginUrls("/installing/repo");
    expect(mocks.execFile).toHaveBeenCalledTimes(1);
    mocks.execFile.mockImplementation((_file, _args, _options, callback) => callback(null, {
      stdout: '{"plugin":["file:///installing/repo/kanvibe-plugin.ts"]}',
    }));
    finish(null, { stdout: '{"plugin":[]}' });
    await expect(oldQuery).resolves.toEqual([]);
    expect(await freshA).toEqual(await freshB);
    expect(await freshA).toHaveLength(1);
    expect(mocks.execFile).toHaveBeenCalledTimes(2);
  });
});
