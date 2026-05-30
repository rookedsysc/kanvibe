import { createRequire } from "node:module";
import { describe, expect, it } from "vitest";

const require = createRequire(import.meta.url);

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
});
