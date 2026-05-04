import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  readFile: vi.fn(),
  mkdir: vi.fn(),
  homedir: vi.fn(() => "/home/local-user"),
}));

vi.mock("fs/promises", () => ({
  default: {
    readFile: mocks.readFile,
    mkdir: mocks.mkdir,
  },
  readFile: mocks.readFile,
  mkdir: mocks.mkdir,
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

describe("sshConfig.buildSSHArgs", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
  });

  it("adds trusted X11 forwarding and forced tty before the SSH destination", async () => {
    // Given
    const { buildSSHArgs } = await import("@/lib/sshConfig");

    // When
    const args = buildSSHArgs({
      host: "remote-host",
      hostname: "example.com",
      port: 2202,
      username: "tester",
      privateKeyPath: "/tmp/test-key",
    }, {
      trustedX11Forwarding: true,
      forceTty: true,
    });

    // Then
    expect(args).toEqual([
      "-i",
      "/tmp/test-key",
      "-p",
      "2202",
      "-o",
      "BatchMode=yes",
      "-o",
      "IdentitiesOnly=yes",
      "-Y",
      "-tt",
      "remote-host",
    ]);
    expect(args.indexOf("-Y")).toBeLessThan(args.indexOf("remote-host"));
    expect(args.indexOf("-tt")).toBeLessThan(args.indexOf("remote-host"));
  });

  it("adds connection reuse options before the SSH destination", async () => {
    // Given
    const { buildSSHArgs } = await import("@/lib/sshConfig");

    // When
    const args = buildSSHArgs({
      host: "remote-host",
      hostname: "example.com",
      port: 2202,
      username: "tester",
      privateKeyPath: "/tmp/test-key",
    }, {
      disableTty: true,
      connectionReuse: {
        controlPath: "/home/local-user/.kanvibe/ssh-%C",
        controlPersist: "10m",
      },
    });

    // Then
    expect(args).toEqual([
      "-i",
      "/tmp/test-key",
      "-p",
      "2202",
      "-o",
      "BatchMode=yes",
      "-o",
      "IdentitiesOnly=yes",
      "-T",
      "-o",
      "ControlMaster=auto",
      "-o",
      "ControlPersist=10m",
      "-o",
      "ControlPath=/home/local-user/.kanvibe/ssh-%C",
      "remote-host",
    ]);
    expect(args.indexOf("ControlPath=/home/local-user/.kanvibe/ssh-%C")).toBeLessThan(args.indexOf("remote-host"));
  });

  it("builds KanVibe connection reuse options under the app-local directory", async () => {
    // Given
    const { getKanvibeSSHConnectionReuseOptions } = await import("@/lib/sshConfig");

    // When
    const result = getKanvibeSSHConnectionReuseOptions();

    // Then
    expect(result).toEqual({
      controlPath: "/home/local-user/.kanvibe/ssh-%C",
      controlPersist: "10m",
    });
  });

  it("creates the KanVibe SSH control directory with private permissions", async () => {
    // Given
    mocks.mkdir.mockResolvedValue(undefined);
    const { ensureKanvibeSSHControlDirectory } = await import("@/lib/sshConfig");

    // When
    await ensureKanvibeSSHControlDirectory();

    // Then
    expect(mocks.mkdir).toHaveBeenCalledWith(
      "/home/local-user/.kanvibe",
      { recursive: true, mode: 0o700 },
    );
  });

  it("detects local X11 availability from DISPLAY", async () => {
    // Given
    const { hasLocalX11Display } = await import("@/lib/sshConfig");

    // When & Then
    expect(hasLocalX11Display({ DISPLAY: ":0" })).toBe(true);
    expect(hasLocalX11Display({})).toBe(false);
  });
});
