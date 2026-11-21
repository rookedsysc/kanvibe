// @vitest-environment node
import { afterEach, describe, expect, it, vi } from "vitest";
import { randomUUID } from "node:crypto";
import { createRequire } from "node:module";
import { RemoteHostPtyRegistry } from "../remoteHostPty";
import type { HostRecord } from "../remoteHostProtocol";

const registries: RemoteHostPtyRegistry[] = [];
afterEach(() => { registries.splice(0).forEach((registry) => registry.close()); vi.restoreAllMocks(); });

function fixture(owner = "desktop-one") {
  const registry = new RemoteHostPtyRegistry(); registries.push(registry);
  const records: HostRecord[] = [];
  const peer = { clientId: owner, send: (record: HostRecord) => { records.push(record); return true; } };
  const open = (id: string = randomUUID(), extra = {}) => {
    registry.receive(peer, { type: "pty.open", id, key: "task-tab", command: "exec /bin/sh", cols: 90, rows: 24, ...extra });
    return records.findLast((record) => record.type === "pty.ready")!;
  };
  const until = async (check: () => boolean) => { await expect.poll(check, { timeout: 4000 }).toBe(true); };
  return { registry, peer, records, open, until };
}

describe("daemon PTY ownership", () => {
  it("reserves attachment capacity before asynchronous screen snapshots complete", () => {
    const f = fixture(); let accepted = 0, rejected = 0;
    for (let index = 0; index < 32; index++) {
      try { f.open(`batch-${index}`); accepted++; } catch (error) { expect(String(error)).toMatch(/capacity/); rejected++; }
    }
    expect({ accepted, rejected }).toEqual({ accepted: 16, rejected: 16 });
  });

  it("rejects attach during exit flush instead of issuing a ready without a terminal outcome", async () => {
    let exit: (event: { exitCode: number }) => void = () => {};
    const native = createRequire(import.meta.url)("node-pty") as typeof import("node-pty");
    vi.spyOn(native, "spawn").mockReturnValue({ pid: 12345, cols: 80, rows: 24, onData: vi.fn(), onExit: (callback: typeof exit) => { exit = callback; }, kill: vi.fn(), pause: vi.fn(), resume: vi.fn() } as never);
    const f = fixture(), first = f.open("first");
    exit({ exitCode: 0 });
    expect(() => f.open("during-close", { sessionId: first.sessionId })).toThrow(/closing|exited/);
    expect(() => f.open("during-close-key")).toThrow(/closing|exited/);
    await f.until(() => f.records.some(r => r.type === "pty.exit" && r.id === "first"));
  });
  it("lists only the owner's detached sessions and closes one explicitly by daemon ID", async () => {
    const f = fixture(); const first = f.open("one");
    f.registry.detach(f.peer);
    f.registry.receive(f.peer, { type: "pty.list", id: "list" });
    expect(f.records.find((r) => r.type === "pty.sessions")?.sessions).toEqual([expect.objectContaining({ sessionId: first.sessionId, pid: first.pid, key: "task-tab" })]);
    const other = { clientId: "another-desktop", send: f.peer.send };
    f.registry.receive(other, { type: "pty.list", id: "other-list" });
    expect(f.records.find((r) => r.id === "other-list")?.sessions).toEqual([]);
    expect(() => f.registry.receive(other, { type: "pty.close", id: "bad-close", sessionId: first.sessionId })).toThrow(/owned|unknown/i);
    f.registry.receive(f.peer, { type: "pty.close", id: "close", sessionId: first.sessionId });
    await f.until(() => { try { process.kill(Number(first.pid), 0); return false; } catch { return true; } });
  });
  it("issues stable session IDs and preserves a real shell PID across detach/reattach", async () => {
    const f = fixture(); const first = f.open("first");
    expect(first.sessionId).toMatch(/^[a-f0-9-]{36}$/); expect(first.pid).toBeGreaterThan(0);
    f.registry.detach(f.peer);
    process.kill(Number(first.pid), 0);
    const second = f.open("second");
    expect(second.sessionId).toBe(first.sessionId); expect(second.pid).toBe(first.pid);
    f.registry.receive(f.peer, { type: "pty.input", id: "second", data: Buffer.from("printf '\\121\\101\\137\\111\\116\\120\\125\\124\\137\\117\\113\\n'\n").toString("base64") });
    await f.until(() => f.records.some((r) => r.type === "pty.data" && Buffer.from(String(r.data), "base64").includes("QA_INPUT_OK")));
    f.registry.receive(f.peer, { type: "pty.resize", id: "second", cols: 111, rows: 37 });
    f.registry.receive(f.peer, { type: "pty.input", id: "second", data: Buffer.from("stty size\n").toString("base64") });
    await f.until(() => f.records.some((r) => r.type === "pty.data" && Buffer.from(String(r.data), "base64").includes("37 111")));
    f.registry.receive(f.peer, { type: "pty.close", id: "second" });
    await f.until(() => f.records.some((r) => r.type === "pty.exit" && r.id === "second"));
  });

  it("isolates owners and never lets another desktop attach or close a session", () => {
    const f = fixture(); const first = f.open("one");
    const other = { clientId: "desktop-two", send: f.peer.send };
    expect(() => f.registry.receive(other, { type: "pty.open", id: "steal", sessionId: first.sessionId, key: "task-tab", command: "exec /bin/sh", cols: 80, rows: 24 })).toThrow(/owned|unknown/i);
    f.registry.receive(other, { type: "pty.close", id: "one" });
    process.kill(Number(first.pid), 0);
    f.registry.receive(other, { type: "pty.open", id: "two", key: "task-tab", command: "exec /bin/sh", cols: 80, rows: 24 });
    expect(f.records.findLast((r) => r.type === "pty.ready")!.sessionId).not.toBe(first.sessionId);
  });

  it("rejects invalid bounds and duplicate attachment IDs without spawning extra sessions", () => {
    const f = fixture(); f.open("one");
    expect(() => f.open("one")).toThrow(/duplicate/i);
    expect(() => f.open("bad", { rows: 0 })).toThrow(/size/i);
    expect(() => f.registry.receive(f.peer, { type: "pty.input", id: "one", data: Buffer.alloc(32769).toString("base64") })).toThrow(/size|bytes|chunk/i);
  });

  it("drains a large raw paste exactly once and acknowledges kernel-consumed chunks", async () => {
    const f = fixture();
    f.open("paste", { command: "stty raw -echo; printf READY; head -c 262144 | wc -c; sleep 10" });
    await f.until(() => f.records.some((r) => r.type === "pty.data" && Buffer.from(String(r.data), "base64").includes("READY")));
    for (let index = 0; index < 8; index++) f.registry.receive(f.peer, { type: "pty.input", id: "paste", data: Buffer.alloc(32768, "x").toString("base64") });
    await f.until(() => f.records.filter((r) => r.type === "pty.input-ready").length === 8);
    await f.until(() => f.records.some((r) => r.type === "pty.data" && Buffer.from(String(r.data), "base64").includes("262144")));
  });

  it("withholds input ACKs while the child does not read and bounds queued paste memory", async () => {
    const f = fixture(); f.open("blocked", { command: "stty raw -echo; printf READY; sleep 30" });
    await f.until(() => f.records.some((r) => r.type === "pty.data" && Buffer.from(String(r.data), "base64").includes("READY")));
    for (let index = 0; index < 32; index++) f.registry.receive(f.peer, { type: "pty.input", id: "blocked", data: Buffer.alloc(32768, "x").toString("base64") });
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(f.records.filter((r) => r.type === "pty.input-ready").length).toBeLessThan(32);
    expect(() => {
      for (let index = 0; index < 257; index++) f.registry.receive(f.peer, { type: "pty.input", id: "blocked", data: Buffer.alloc(32768, "x").toString("base64") });
    }).toThrow(/backpressure/);
    f.registry.detach(f.peer);
  });

  it("detaches a consumer that does not ACK output while preserving the daemon-owned session", async () => {
    const f = fixture(); const first = f.open("slow", { command: "head -c 2097152 /dev/zero | tr '\\000' x; sleep 30" });
    await f.until(() => f.records.some((r) => r.type === "pty.detached"));
    process.kill(Number(first.pid), 0);
    const attached = f.open("again"); expect(attached.pid).toBe(first.pid); expect(attached.sessionId).toBe(first.sessionId);
  });
});
