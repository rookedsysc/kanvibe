// @vitest-environment node
import { afterEach, describe, expect, it } from "vitest";
import { mkdtemp, rm } from "node:fs/promises";
import { connect, type Socket } from "node:net";
import { startRemoteHostServer } from "../remoteHostServer";
import { createHostRecordDecoder, encodeHostRecord, type HostRecord } from "../remoteHostProtocol";

const cleanup: Array<() => Promise<unknown> | void> = [];
afterEach(async () => { for (const close of cleanup.reverse()) await close(); cleanup.length = 0; });
async function fixture() {
  const root = await mkdtemp("/tmp/kvpty-wire-"); cleanup.push(() => rm(root, { recursive: true, force: true }));
  const server = await startRemoteHostServer({ namespace: "qa-pty-wire", buildHash: "a".repeat(64), rootDirectory: root }); cleanup.push(() => server.close());
  const peer = async (clientId: string) => {
    const socket: Socket = connect(server.paths.rpcSocketPath); cleanup.push(() => { socket.destroy(); });
    const records: HostRecord[] = [];
    socket.on("data", createHostRecordDecoder((record) => records.push(record)));
    socket.on("error", () => {});
    const next = async (type?: string) => {
      await expect.poll(() => records.findIndex((r) => !type || r.type === type), { timeout: 4000 }).toBeGreaterThanOrEqual(0);
      return records.splice(records.findIndex((r) => !type || r.type === type), 1)[0];
    };
    const send = (record: HostRecord) => socket.write(encodeHostRecord(record));
    send({ type: "hello", version: 1, namespace: "qa-pty-wire", clientId, role: "desktop", buildHash: "a".repeat(64) });
    await next("ready"); return { socket, records, next, send };
  };
  return { peer };
}
describe("PTY over the authenticated host wire", () => {
  it("opens a real daemon PTY and reattaches the same PID after SSH attachment loss", async () => {
    const f = await fixture(); const a = await f.peer("desktop-one");
    const request = { type: "pty.open", id: "first", key: "stable-tab", command: "exec /bin/sh", cols: 80, rows: 24 };
    a.send(request);
    const first = await a.next();
    expect(first.type).toBe("pty.ready");
    a.socket.destroy();
    await new Promise<void>((resolve) => a.socket.once("close", resolve));
    const b = await f.peer("desktop-one"); b.send({ ...request, id: "second" });
    const second = await b.next("pty.ready");
    expect(second.pid).toBe(first.pid); expect(second.sessionId).toBe(first.sessionId);
    b.send({ type: "pty.input", id: "second", data: Buffer.from("printf '\\127\\111\\122\\105\\137\\117\\113\\n'\n").toString("base64") });
    await b.next("pty.input-ready");
    await expect.poll(() => b.records.some((r) => r.type === "pty.data" && Buffer.from(String(r.data), "base64").includes("WIRE_OK"))).toBe(true);
    b.send({ type: "pty.close", id: "second" }); expect((await b.next("pty.exit")).sessionId).toBe(first.sessionId);
  });
});
