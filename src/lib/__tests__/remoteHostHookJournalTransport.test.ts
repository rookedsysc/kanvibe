// @vitest-environment node
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mkdtemp, rm } from "node:fs/promises";
import { connect } from "node:net";
import { request } from "node:http";
import { EventEmitter } from "node:events";
import { PassThrough } from "node:stream";
import { startRemoteHostServer } from "../remoteHostServer";
import { HostConnection, type HostProcess } from "../remoteHostConnection";
import { createHostRecordDecoder, encodeHostRecord, type HostRecord } from "../remoteHostProtocol";

describe("durable hooks across actual host sockets", () => {
  let rootDirectory: string;
  let server: Awaited<ReturnType<typeof startRemoteHostServer>>;
  const connections: HostConnection[] = [];
  const start = () => startRemoteHostServer({ namespace: "qa-journal", buildHash: "a".repeat(64), rootDirectory });
  beforeEach(async () => { rootDirectory = await mkdtemp("/tmp/kvhjt-"); server = await start(); });
  afterEach(async () => { connections.forEach(c => c.close()); connections.length = 0; await server.close(); await rm(rootDirectory, { recursive: true, force: true }); });
  function post(clientId: string, status = "review", url = "/api/hooks/status", raw?: string, headers: Record<string,string> = {}) {
    return new Promise<{ status: number; body: string }>((resolve, reject) => {
      const req = request({ socketPath: server.paths.hookSocketPath, method: "POST", path: url, headers: { "x-kanvibe-client-id": clientId, ...headers } }, response => {
        let body = ""; response.on("data", chunk => { body += chunk; }); response.once("end", () => resolve({ status: response.statusCode!, body }));
      });
      req.once("error", reject); req.end(raw ?? JSON.stringify({ taskId: "fixture", status }));
    });
  }
  async function attach(clientId = "client-a") {
    const socket = connect(server.paths.rpcSocketPath);
    const child = Object.assign(new EventEmitter(), { stdin: socket, stdout: socket, stderr: new PassThrough(), kill: () => { socket.destroy(); return true; } }) as HostProcess;
    const connection = new HostConnection(child); connections.push(connection);
    const statuses = new Map<string, (record: HostRecord) => void>();
    await new Promise<void>((resolve, reject) => {
      socket.on("error", reject);
      socket.on("data", createHostRecordDecoder(record => {
        if (record.type === "ready") resolve();
        else if (record.type === "status") { statuses.get(String(record.id))?.(record); statuses.delete(String(record.id)); }
        else connection.receive(record);
      }));
      socket.write(encodeHostRecord({ type: "hello", version: 1, namespace: "qa-journal", buildHash: "a".repeat(64), role: "desktop", clientId }));
    });
    socket.on("close", () => connection.close());
    let sequence = 0;
    return { connection, status: () => new Promise<HostRecord>(resolve => { const id = "status-" + sequence++; statuses.set(id, resolve); connection.send({ type: "status", id }); }) };
  }
  const ack = (delivery = "applied") => ({ status: 200, headers: { "content-type": "application/json" }, body: Buffer.from(JSON.stringify({ success: true, delivery })).toString("base64") });

  it("accepts offline events durably and replays FIFO after server restart and handler installation", async () => {
    expect((await post("client-a", "progress")).status).toBe(202);
    expect((await post("client-a", "done")).status).toBe(202);
    expect((await post("client-b", "review")).status).toBe(202);
    await server.close(); server = await start();
    const a = await attach(); const received: HostRecord[] = [];
    expect((await a.status()).journal).toMatchObject({ pending: 3 });
    a.connection.setHookHandler(async event => { received.push(event); return ack(); });
    await vi.waitFor(() => expect(received).toHaveLength(2));
    expect(received.map(event => JSON.parse(Buffer.from(String(event.body), "base64").toString()).status)).toEqual(["progress", "done"]);
    expect(received[0].journal).toMatchObject({ sequence: 1, replay: true });
    expect((await a.status()).journal).toMatchObject({ pending: 1 });
    const b = await attach("client-b"); const other: HostRecord[] = [];
    b.connection.setHookHandler(async event => { other.push(event); return ack("ignored"); });
    await vi.waitFor(async () => expect((await b.status()).journal).toMatchObject({ pending: 0, ignored: 1 }));
    expect(other).toHaveLength(1);
  });

  it("keeps a lost ACK and delivers the same event id on reconnect", async () => {
    await post("client-a");
    const a = await attach(); let first: HostRecord | undefined;
    a.connection.setHookHandler(async event => { first = event; await new Promise<void>(resolve => a.connection.once("close", resolve)); return ack(); });
    await vi.waitFor(() => expect(first).toBeDefined());
    a.connection.close();
    const b = await attach(); const retried: HostRecord[] = [];
    b.connection.setHookHandler(async event => { retried.push(event); return ack("ignored"); });
    await vi.waitFor(async () => expect((await b.status()).journal).toMatchObject({ pending: 0, ignored: 1 }));
    expect(retried[0].id).toBe(first!.id);
    expect(retried[0].journal).toMatchObject({ replay: true });
  });

  it("retains transient delivery failures, rejects permanent failures and preserves live HTTP responses", async () => {
    const a = await attach();
    a.connection.setHookHandler(async () => ({ status: 503, headers: {}, body: "" }));
    expect((await post("client-a")).status).toBe(503);
    expect((await a.status()).journal).toMatchObject({ pending: 1 });
    a.connection.setHookHandler(async () => ({ status: 404, headers: {}, body: "" }));
    await vi.waitFor(async () => expect((await a.status()).journal).toMatchObject({ pending: 0, rejected: 1 }));
    a.connection.setHookHandler(async event => { expect(event.journal).toMatchObject({ replay: false }); return ack(); });
    expect(await post("client-a", "done")).toEqual({ status: 200, body: JSON.stringify({ success: true, delivery: "applied" }) });
  });

  it("does not journal offline Task CLI mutations or accept malformed and browser hooks", async () => {
    expect((await post("client-a", "done", "/api/cli/tasks", "{}")).status).toBe(503);
    expect((await post("client-a", "done", "/api/hooks/status", "{bad")).status).toBe(400);
    expect((await post("bad/client")).status).toBe(400);
    expect((await post("client-a", "done", "/api/hooks/status", undefined, { Origin: "https://untrusted.test" })).status).toBe(403);
    const a = await attach();
    expect((await a.status()).journal).toMatchObject({ pending: 0 });
  });

  it("bounds unfinished HTTP bodies without blocking RPC and releases abandoned reservations", async () => {
    const a = await attach();
    const pending = Array.from({ length: 128 }, () => {
      const req = request({ socketPath: server.paths.hookSocketPath, method: "POST", path: "/api/hooks/status", headers: { "x-kanvibe-client-id": "client-a", "content-length": "100" } });
      req.on("error", () => undefined); req.write("{"); return req;
    });
    try {
      await vi.waitFor(async () => expect((await a.status()).journal).toMatchObject({ activeRequests: 128 }));
      expect((await post("client-b")).status).toBe(503);
      expect(await a.connection.execute("printf alive")).toBe("alive");
    } finally { pending.forEach(req => req.destroy()); }
    await vi.waitFor(async () => expect((await a.status()).journal).toMatchObject({ activeRequests: 0 }));
    expect((await post("client-b")).status).toBe(202);
  });

  it("does not acknowledge an application-level failure hidden behind HTTP 200", async () => {
    const a = await attach();
    a.connection.setHookHandler(async () => ({ status: 200, headers: {}, body: Buffer.from('{"success":false}').toString("base64") }));
    expect((await post("client-a")).status).toBe(200);
    expect((await a.status()).journal).toMatchObject({ pending: 1 });
    a.connection.setHookHandler(async () => ack());
    await vi.waitFor(async () => expect((await a.status()).journal).toMatchObject({ pending: 0 }));
  });
});
