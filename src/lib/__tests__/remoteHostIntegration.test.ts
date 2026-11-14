// @vitest-environment node
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mkdtemp, rm } from "node:fs/promises";
import { connect, createServer, type Socket } from "node:net";
import { request } from "node:http";
import { EventEmitter } from "node:events";
import { PassThrough } from "node:stream";
import { startRemoteHostServer } from "@/lib/remoteHostServer";
import { HostConnection, type HostProcess } from "@/lib/remoteHostConnection";
import { createHostRecordDecoder, encodeHostRecord } from "@/lib/remoteHostProtocol";

describe("real host provider and client", () => {
  let directory: string;
  let server: Awaited<ReturnType<typeof startRemoteHostServer>>;
  const connections: HostConnection[] = [];
  beforeEach(async () => {
    directory = await mkdtemp("/tmp/kvhi-");
    server = await startRemoteHostServer({ namespace: "qa-integration", buildHash: "a".repeat(64), rootDirectory: directory });
  });
  afterEach(async () => {
    connections.splice(0).forEach((connection) => connection.close());
    await server?.close();
    await rm(directory, { recursive: true, force: true });
  });
  async function attach(clientId = "client-one") {
    const socket = connect(server.paths.rpcSocketPath);
    const child = Object.assign(new EventEmitter(), {
      stdin: socket, stdout: socket, stderr: new PassThrough(),
      kill: () => { socket.destroy(); return true; },
    }) as HostProcess;
    const connection = new HostConnection(child);
    connections.push(connection);
    await new Promise<void>((resolve, reject) => {
      socket.on("error", reject);
      socket.on("data", createHostRecordDecoder((record) => {
        if (record.type === "ready") { connection.instanceId = String(record.instanceId); resolve(); }
        else connection.receive(record);
      }));
      socket.write(encodeHostRecord({ type: "hello", version: 1, namespace: "qa-integration", role: "desktop", clientId, buildHash: "a".repeat(64) }));
    });
    socket.once("close", () => connection.close());
    return connection;
  }
  function hook(clientId: string, body: string): Promise<{ status: number; body: string }> {
    return new Promise((resolve, reject) => {
      const req = request({ socketPath: server.paths.hookSocketPath, path: "/api/hooks/status", method: "POST", headers: { "x-kanvibe-client-id": clientId } }, (response) => {
        let result = ""; response.on("data", (chunk) => { result += chunk; });
        response.on("end", () => resolve({ status: response.statusCode!, body: result }));
      });
      req.once("error", reject); req.end(body);
    });
  }
  it("delivers an 8MiB file transaction response without replay or truncation", async () => {
    const connection = await attach();
    const response = Buffer.alloc(8 * 1024 * 1024, 0x61).toString("base64") + "\n";
    let calls = 0;
    const result = await connection.execute("printf 'READY\\tpayload\\n'; wc -c", {
      timeoutMs: 15_000,
      remoteStdinResponder: { marker: "READY", respond: async (payload) => { expect(payload).toBe("payload"); calls += 1; return response; } },
    });
    expect(Number(result)).toBe(Buffer.byteLength(response));
    expect(calls).toBe(1);
  }, 20_000);
  it("keeps hooks and another client progressing during twenty bulk results", async () => {
    const first = await attach(), second = await attach("client-two");
    const received: string[] = [];
    second.setHookHandler(async (record) => { received.push(Buffer.from(String(record.body), "base64").toString()); return { status: 200, headers: { "content-type": "text/plain" }, body: Buffer.from("ack").toString("base64") }; });
    const bulk = Array.from({ length: 20 }, () => first.execute("head -c 131072 /dev/zero", { timeoutMs: 10_000 }));
    expect(await hook("client-two", '{"taskId":"fixture","status":"review"}')).toEqual({ status: 200, body: "ack" });
    expect(await second.execute("printf interactive")).toBe("interactive");
    const outputs = await Promise.all(bulk);
    expect(outputs.every((output) => output.length === 131072)).toBe(true);
    expect(received).toEqual(['{"taskId":"fixture","status":"review"}']);
  });
  it("forwards binary native bytes and detach preserves its upstream server", async () => {
    const connection = await attach();
    const upstreamPath = `${directory}/native.sock`;
    const upstreamSockets = new Set<Socket>();
    const upstream = createServer((socket) => { upstreamSockets.add(socket); socket.on("close", () => upstreamSockets.delete(socket)); socket.pipe(socket); });
    await new Promise<void>((resolve) => upstream.listen(upstreamPath, resolve));
    let forward: Awaited<ReturnType<HostConnection["forwardSocket"]>> | undefined;
    try {
      forward = await connection.forwardSocket(upstreamPath);
      const socket = connect(forward.socketPath);
      const bytes = Buffer.alloc(96 * 1024, 0xff); bytes[0] = 0;
      const actual = await new Promise<Buffer>((resolve, reject) => {
        const chunks: Buffer[] = []; let length = 0;
        socket.on("data", (chunk) => { chunks.push(chunk); length += chunk.length; if (length === bytes.length) { socket.end(); resolve(Buffer.concat(chunks)); } });
        socket.on("error", reject); socket.write(bytes);
      });
      expect(actual.equals(bytes)).toBe(true);
      await forward.close(); forward = undefined;
      connection.close();
      expect(upstream.listening).toBe(true);
      const next = await attach("client-new");
      expect(next.instanceId).toBe(server.instanceId);
    } finally {
      await forward?.close();
      upstreamSockets.forEach((socket) => socket.destroy());
      await new Promise<void>((resolve) => upstream.close(() => resolve()));
    }
  });
  it("retains offline status hooks and replays after handler installation", async () => {
    const first = await attach(); first.close();
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect((await hook("client-one", '{"taskId":"fixture","status":"review"}')).status).toBe(202);
    const second = await attach(); let events = 0;
    second.setHookHandler(async () => { events += 1; return { status: 200, headers: {}, body: "" }; });
    expect(await second.execute("printf snapshot")).toBe("snapshot");
    await vi.waitFor(() => expect(events).toBe(1));
  });
});
