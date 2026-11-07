import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { WebContents } from "electron";
import { closeAiAccountLogin, closeWindowAiAccountLogins, openAiAccountLogin, writeAiAccountLogin } from "../aiAccountLoginBridge";

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
