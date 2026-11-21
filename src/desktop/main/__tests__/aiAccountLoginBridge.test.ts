import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { WebContents } from "electron";
import { closeAiAccountLogin, closeWindowAiAccountLogins, openAiAccountLogin, resizeAiAccountLogin, writeAiAccountLogin } from "../aiAccountLoginBridge";

const { spawn } = vi.hoisted(() => ({ spawn: vi.fn() }));
vi.mock("node-pty", () => ({ spawn }));
vi.mock("@/lib/aiUsage/providerCli", () => ({ createProviderCliEnvironment: () => ({ HOME: "/tmp/account" }), getProviderLoginCommand: () => ({ command: "synthetic-cli", args: [] }) }));
const contents = { id: 91, isDestroyed: vi.fn(() => false), send: vi.fn() } as unknown as WebContents;
function pty() {
  return { onData: vi.fn(), onExit: vi.fn(), kill: vi.fn(), write: vi.fn(), resize: vi.fn() };
}
beforeEach(() => { vi.clearAllMocks(); });
afterEach(() => closeWindowAiAccountLogins(contents.id));

describe("login terminal initialization ownership", () => {
  it.each(["resize", "write", "kill"] as const)("survives %s after the native PTY closes before onExit", async (operation) => {
    const process = pty();
    spawn.mockReturnValue(process);
    await openAiAccountLogin(contents, "claude", "/tmp/account", 80, 24);
    process[operation].mockImplementation(() => { throw new Error("ioctl(2) failed, EBADF"); });
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    try {
      expect(() => {
        if (operation === "resize") resizeAiAccountLogin(contents.id, "/tmp/account", 100, 30);
        else if (operation === "write") writeAiAccountLogin(contents.id, "/tmp/account", "code");
        else closeAiAccountLogin(contents.id, "/tmp/account");
      }).not.toThrow();
      process.onExit.mock.calls[0][0]({ exitCode: 1 });
      spawn.mockReturnValue(pty());
      expect(await openAiAccountLogin(contents, "claude", "/tmp/account", 80, 24)).toEqual({ ok: true });
    } finally {
      warn.mockRestore();
    }
  });

  it("continues closing other logins when one native kill fails", async () => {
    const first = pty();
    const second = pty();
    spawn.mockReturnValueOnce(first).mockReturnValueOnce(second);
    await openAiAccountLogin(contents, "claude", "/tmp/account", 80, 24);
    await openAiAccountLogin(contents, "codex", "/tmp/second", 80, 24);
    first.kill.mockImplementation(() => { throw new Error("EBADF"); });
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    try {
      expect(() => closeWindowAiAccountLogins(contents.id)).not.toThrow();
      expect(second.kill).toHaveBeenCalledOnce();
    } finally {
      warn.mockRestore();
    }
  });

  it("forwards live dimensions and ignores resize after the exit notification", async () => {
    const process = pty();
    spawn.mockReturnValue(process);
    await openAiAccountLogin(contents, "claude", "/tmp/account", 80, 24);
    resizeAiAccountLogin(contents.id, "/tmp/account", 100, 30);
    expect(process.resize).toHaveBeenCalledWith(100, 30);
    process.onExit.mock.calls[0][0]({ exitCode: 0 });
    resizeAiAccountLogin(contents.id, "/tmp/account", 120, 40);
    expect(process.resize).toHaveBeenCalledTimes(1);
    expect(contents.send).toHaveBeenCalledWith("kanvibe:ai-login-exit", { accountRoot: "/tmp/account", exitCode: 0 });
  });
  it("shares concurrent opens and kills the sole PTY on close", async () => {
    const process = pty();
    spawn.mockReturnValue(process);
    const results = await Promise.all([openAiAccountLogin(contents, "claude", "/tmp/account", 80, 24), openAiAccountLogin(contents, "claude", "/tmp/account", 80, 24)]);
    expect(results).toEqual([{ ok: true }, { ok: true }]);
    expect(spawn).toHaveBeenCalledTimes(1);
    closeAiAccountLogin(contents.id, "/tmp/account");
    expect(process.kill).toHaveBeenCalledTimes(1);
  });

  it("cancels a close while module loading is pending", async () => {
    spawn.mockReturnValue(pty());
    const pending = openAiAccountLogin(contents, "claude", "/tmp/account", 80, 24);
    closeAiAccountLogin(contents.id, "/tmp/account");
    expect(await pending).toEqual({ ok: false });
    expect(spawn).not.toHaveBeenCalled();
  });

  it("ignores stale exit callbacks after a replacement login opens", async () => {
    const first = pty();
    const second = pty();
    spawn.mockReturnValueOnce(first).mockReturnValueOnce(second);
    await openAiAccountLogin(contents, "claude", "/tmp/account", 80, 24);
    closeAiAccountLogin(contents.id, "/tmp/account");
    await openAiAccountLogin(contents, "claude", "/tmp/account", 80, 24);
    first.onExit.mock.calls[0][0]({ exitCode: 0 });
    writeAiAccountLogin(contents.id, "/tmp/account", "hello");
    expect(second.write).toHaveBeenCalledWith("hello");
    expect(contents.send).not.toHaveBeenCalled();
  });

  it("cancels a window's pending opens", async () => {
    spawn.mockReturnValue(pty());
    const pending = openAiAccountLogin(contents, "claude", "/tmp/account", 80, 24);
    closeWindowAiAccountLogins(contents.id);
    expect(await pending).toEqual({ ok: false });
    expect(spawn).not.toHaveBeenCalled();
  });
});
