import { describe, expect, it } from "vitest";
import { extractRegisteredKanvibePluginUrls, extractRegisteredPluginUrls, isKanvibePluginUrl } from "../openCodePluginRegistry";

describe("openCodePluginRegistry", () => {
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
});
