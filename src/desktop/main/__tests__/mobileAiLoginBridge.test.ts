import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { closeAllMobileAiLogins, closeMobileAiLogin, closeMobileAiLoginsForAccount, closeMobileAiLoginsForDevice, openMobileAiLoginSession, resizeMobileAiLogin, writeMobileAiLogin } from "../mobileAiLoginBridge";
const mocks = vi.hoisted(() => ({ spawn: vi.fn(), environment: vi.fn(), command: vi.fn() }));
vi.mock("node-pty", () => ({ spawn: mocks.spawn }));
vi.mock("@/lib/aiUsage/providerCli", () => ({ createProviderCliEnvironment: mocks.environment, getProviderLoginCommand: mocks.command }));
function createPty() { return { onData: vi.fn(() => ({ dispose: vi.fn() })), onExit: vi.fn(() => ({ dispose: vi.fn() })), kill: vi.fn(), write: vi.fn(), resize: vi.fn() }; }
function request(overrides = {}) { return { deviceId: "device-1", socketId: "socket-1", provider: "claude" as const, accountRoot: "/fixture/account", cols: 80, rows: 24, isOpen: () => true, onEvent: vi.fn(), ...overrides }; }
beforeEach(() => {
  vi.resetAllMocks();
  mocks.environment.mockReturnValue({ HOME: "/fixture", CLAUDE_CONFIG_DIR: "/fixture/account" });
  mocks.command.mockReturnValue({ command: "synthetic-cli", args: ["login"] });
  mocks.spawn.mockImplementation(() => createPty());
});
afterEach(() => { closeAllMobileAiLogins(); vi.useRealTimers(); });
describe("mobile login PTY ownership", () => {
  it("opens the existing provider CLI with scoped environment, emits output and terminal exit", async () => {
    const options = request();
    const before = { ...process.env };
    await openMobileAiLoginSession(options);
    expect(mocks.spawn).toHaveBeenCalledWith("synthetic-cli", ["login"], { name: "xterm-color", cols: 80, rows: 24, cwd: "/fixture", env: { HOME: "/fixture", CLAUDE_CONFIG_DIR: "/fixture/account" } });
    expect(process.env).toEqual(before);
    const pty = mocks.spawn.mock.results[0].value;
    pty.onData.mock.calls[0][0]("enter code:");
    pty.onExit.mock.calls[0][0]({ exitCode: 0 });
    expect(options.onEvent.mock.calls).toEqual([[{ type: "output", data: "enter code:" }], [{ type: "exit", exitCode: 0 }]]);
    writeMobileAiLogin("device-1", "socket-1", "ignored");
    expect(pty.write).not.toHaveBeenCalled();
  });
  it("uses Codex device auth for phone browsers without changing other provider commands", async () => {
    mocks.command.mockReturnValue({ command: "codex", args: ["login"] });
    await openMobileAiLoginSession(request({ provider: "codex" }));
    expect(mocks.spawn).toHaveBeenCalledWith("codex", ["login", "--device-auth"], expect.any(Object));
    expect(mocks.command.mock.results[0].value.args).toEqual(["login"]);
  });
  it("isolates input, resize and close to the device/socket that owns the PTY", async () => {
    await openMobileAiLoginSession(request());
    const pty = mocks.spawn.mock.results[0].value;
    writeMobileAiLogin("device-2", "socket-1", "attack");
    resizeMobileAiLogin("device-1", "another-socket", 90, 30);
    closeMobileAiLogin("device-2", "socket-1");
    expect(pty.write).not.toHaveBeenCalled();
    expect(pty.resize).not.toHaveBeenCalled();
    expect(pty.kill).not.toHaveBeenCalled();
    writeMobileAiLogin("device-1", "socket-1", "code\r");
    resizeMobileAiLogin("device-1", "socket-1", 90, 30);
    expect(pty.write).toHaveBeenCalledWith("code\r");
    expect(pty.resize).toHaveBeenCalledWith(90, 30);
    closeMobileAiLogin("device-1", "socket-1");
    expect(pty.kill).toHaveBeenCalledTimes(1);
  });
  it("prevents concurrent login writers for the same account", async () => {
    await openMobileAiLoginSession(request());
    await expect(openMobileAiLoginSession(request({ deviceId: "device-2", socketId: "socket-2" }))).rejects.toThrow("로그인");
    expect(mocks.spawn).toHaveBeenCalledTimes(1);
  });
  it("does not spawn after socket closure during async initialization", async () => {
    mocks.spawn.mockClear();
    const opening = openMobileAiLoginSession(request());
    closeMobileAiLogin("device-1", "socket-1");
    await opening;
    expect(mocks.spawn).not.toHaveBeenCalled();
  });
  it("closes only the unpaired device's pending and running processes", async () => {
    await openMobileAiLoginSession(request());
    await openMobileAiLoginSession(request({ deviceId: "device-2", socketId: "socket-2", accountRoot: "/fixture/second" }));
    closeMobileAiLoginsForDevice("device-1");
    expect(mocks.spawn.mock.results[0].value.kill).toHaveBeenCalledTimes(1);
    expect(mocks.spawn.mock.results[1].value.kill).not.toHaveBeenCalled();
    writeMobileAiLogin("device-2", "socket-2", "still active");
    expect(mocks.spawn.mock.results[1].value.write).toHaveBeenCalledWith("still active");
  });
  it("kills all account owners before the account is removed", async () => {
    await openMobileAiLoginSession(request());
    closeMobileAiLoginsForAccount("/fixture/account");
    expect(mocks.spawn.mock.results[0].value.kill).toHaveBeenCalledTimes(1);
  });
  it("expires abandoned logins and makes ownership reusable", async () => {
    vi.useFakeTimers();
    const options = request();
    await openMobileAiLoginSession(options);
    await vi.advanceTimersByTimeAsync(10 * 60 * 1000);
    expect(mocks.spawn.mock.results[0].value.kill).toHaveBeenCalledTimes(1);
    expect(options.onEvent).toHaveBeenCalledWith({ type: "error", error: expect.stringContaining("만료") });
    await openMobileAiLoginSession(request());
    expect(mocks.spawn).toHaveBeenCalledTimes(2);
  });
  it("sanitizes CLI startup failures and releases the account lock", async () => {
    mocks.spawn.mockImplementationOnce(() => { throw new Error("secret /home/user/token"); });
    await expect(openMobileAiLoginSession(request())).rejects.toThrow("로그인 터미널");
    await openMobileAiLoginSession(request());
    expect(mocks.spawn).toHaveBeenCalledTimes(2);
  });
  it("cleans pending opens when a device unpairs and clears every session on shutdown", async () => {
    const opening = openMobileAiLoginSession(request());
    closeMobileAiLoginsForDevice("device-1");
    await opening;
    expect(mocks.spawn).not.toHaveBeenCalled();
    await openMobileAiLoginSession(request());
    await openMobileAiLoginSession(request({ deviceId: "device-2", socketId: "socket-2", accountRoot: "/fixture/second" }));
    closeAllMobileAiLogins();
    expect(mocks.spawn.mock.results.map((result) => result.value.kill.mock.calls.length)).toEqual([1, 1]);
  });
  it("rejects invalid viewport sizes and oversized input before forwarding to the PTY", async () => {
    await expect(openMobileAiLoginSession(request({ cols: -1 }))).rejects.toMatchObject({ statusCode: 400 });
    await openMobileAiLoginSession(request());
    const pty = mocks.spawn.mock.results[0].value;
    expect(() => writeMobileAiLogin("device-1", "socket-1", "x".repeat(65537))).toThrow();
    expect(() => resizeMobileAiLogin("device-1", "socket-1", 80, 251)).toThrow();
    expect(pty.write).not.toHaveBeenCalled();
    expect(pty.resize).not.toHaveBeenCalled();
  });
  it("ignores stale output/exit callbacks after a replacement session opens", async () => {
    const first = request();
    await openMobileAiLoginSession(first);
    const pty = mocks.spawn.mock.results[0].value;
    closeMobileAiLogin("device-1", "socket-1");
    const next = request();
    await openMobileAiLoginSession(next);
    pty.onData.mock.calls[0][0]("stale");
    pty.onExit.mock.calls[0][0]({ exitCode: 0 });
    writeMobileAiLogin("device-1", "socket-1", "new");
    expect(first.onEvent).not.toHaveBeenCalled();
    expect(mocks.spawn.mock.results[1].value.write).toHaveBeenCalledWith("new");
  });
});
