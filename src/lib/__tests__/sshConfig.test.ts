import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  readFile: vi.fn(),
  homedir: vi.fn(() => "/home/local-user"),
}));

vi.mock("fs/promises", () => ({
  default: {
    readFile: mocks.readFile,
  },
  readFile: mocks.readFile,
}));

vi.mock("os", () => ({
  default: {
    homedir: mocks.homedir,
  },
  homedir: mocks.homedir,
}));

describe("sshConfig.parseSSHConfig", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
  });

  it("expands concrete host aliases and skips wildcard aliases", async () => {
    mocks.readFile.mockResolvedValue(`Host app-prod app-bastion *.internal\n  HostName example.com\n  User tester\n  Port 2202\n  IdentityFile ~/.ssh/custom_key\n`);

    const { parseSSHConfig } = await import("@/lib/sshConfig");

    await expect(parseSSHConfig()).resolves.toEqual([
      {
        host: "app-prod",
        hostname: "example.com",
        port: 2202,
        username: "tester",
        privateKeyPath: "/home/local-user/.ssh/custom_key",
      },
      {
        host: "app-bastion",
        hostname: "example.com",
        port: 2202,
        username: "tester",
        privateKeyPath: "/home/local-user/.ssh/custom_key",
      },
    ]);
  });

  it("reuses parsed SSH config on repeated calls", async () => {
    mocks.readFile.mockResolvedValue(`Host app-prod\n  HostName example.com\n  User tester\n`);

    const { parseSSHConfig } = await import("@/lib/sshConfig");

    await parseSSHConfig();
    await parseSSHConfig();

    expect(mocks.readFile).toHaveBeenCalledTimes(1);
  });
});
