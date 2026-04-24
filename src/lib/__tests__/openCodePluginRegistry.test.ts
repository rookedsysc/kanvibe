import { describe, expect, it } from "vitest";
import { extractRegisteredPluginUrls } from "../openCodePluginRegistry";

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
});
