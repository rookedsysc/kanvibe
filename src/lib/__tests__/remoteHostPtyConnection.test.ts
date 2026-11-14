// @vitest-environment node
import { describe, expect, it } from "vitest";
import { openRemoteHostPty } from "../remoteHostPtyConnection";
import type { HostRecord } from "../remoteHostProtocol";

function fixture() {
  const records: HostRecord[] = [];
  let receive: (record: HostRecord) => void = () => {};
  const pending = openRemoteHostPty((record) => { records.push(record); return true; }, (_id, handler) => { receive = handler; return () => { receive = () => {}; }; }, { key: "tab", command: "exec sh", cols: 80, rows: 24 });
  const id = records[0].id;
  const send = (record: HostRecord) => receive({ id, ...record });
  send({ type: "pty.ready", sessionId: "daemon-issued", pid: 1234, cols: 80, rows: 24 });
  return { records, pending, send };
}
describe("PTY attachment adapter", () => {
  it("delivers final buffered output before exit even when both arrive before listener registration", async () => {
    const f = fixture(); f.send({ type: "pty.data", data: Buffer.from("last output").toString("base64") }); f.send({ type: "pty.exit", exitCode: 7 });
    const pty = await f.pending, output: string[] = [], exits: number[] = [];
    let rendered = () => {};
    pty.onData(data => { output.push(data); return new Promise<void>(resolve => { rendered = resolve; }); });
    pty.onExit(event => { exits.push(event.exitCode); });
    expect(output).toEqual(["last output"]); expect(exits).toEqual([]);
    rendered(); await Promise.resolve(); await Promise.resolve();
    expect(exits).toEqual([7]);
  });
  it("buffers same-tick ready/snapshot bytes without ACK and waits for backlog rendering", async () => {
    const f = fixture();
    f.send({ type: "pty.data", data: Buffer.from("screen").toString("base64") });
    expect(f.records.some(r => r.type === "pty.ack")).toBe(false);
    const pty = await f.pending;
    let rendered = () => {};
    pty.onData(() => new Promise<void>(resolve => { rendered = resolve; }));
    expect(f.records.some(r => r.type === "pty.ack")).toBe(false);
    rendered(); await Promise.resolve(); await Promise.resolve();
    expect(f.records.at(-1)).toMatchObject({ type: "pty.ack", bytes: 6 });
    pty.kill();
  });

  it("serializes buffered split UTF8 and live frames across concurrent consumers and reverse render completion", async () => {
    const f = fixture(), bytes = Buffer.from("한글");
    f.send({ type: "pty.data", data: bytes.subarray(0, 1).toString("base64") });
    f.send({ type: "pty.data", data: bytes.subarray(1).toString("base64") });
    const pty = await f.pending;
    const first: string[] = [], second: string[] = [];
    const renders: Array<() => void> = [];
    pty.onData(data => { first.push(data); return new Promise<void>(resolve => renders.push(resolve)); });
    pty.onData(data => { second.push(data); return new Promise<void>(resolve => renders.push(resolve)); });
    f.send({ type: "pty.data", data: Buffer.from("live").toString("base64") });
    expect(first).toEqual([""]); expect(second).toEqual([]);
    renders.shift()!(); await Promise.resolve(); await Promise.resolve();
    expect(f.records.filter(r => r.type === "pty.ack").map(r => r.bytes)).toEqual([1]);
    expect(first).toEqual(["", "한글"]); expect(second).toEqual(["한글"]);
    renders.pop()!(); await Promise.resolve(); await Promise.resolve();
    expect(f.records.filter(r => r.type === "pty.ack")).toHaveLength(1);
    renders.shift()!(); await Promise.resolve(); await Promise.resolve();
    expect(f.records.filter(r => r.type === "pty.ack").map(r => r.bytes)).toEqual([1, 5]);
    expect(first.at(-1)).toBe("live"); expect(second.at(-1)).toBe("live");
    renders.pop()!(); renders.shift()!(); await Promise.resolve(); await Promise.resolve();
    expect(f.records.filter(r => r.type === "pty.ack").map(r => r.bytes)).toEqual([1, 5, 4]);
    pty.kill();
  });

  it("detaches without acknowledging buffered output when its renderer rejects", async () => {
    const f = fixture(); f.send({ type: "pty.data", data: Buffer.from("buffered").toString("base64") });
    const pty = await f.pending;
    pty.onData(async () => { throw new Error("render failed"); });
    await Promise.resolve(); await Promise.resolve(); await Promise.resolve();
    expect(f.records.some(r => r.type === "pty.ack")).toBe(false);
    expect(f.records.at(-1)?.type).toBe("pty.detach");
  });
  it("withholds output ACK until its screen consumer has rendered the chunk", async () => {
    const f = fixture(), pty = await f.pending;
    let rendered = () => {};
    pty.onData(() => new Promise<void>(resolve => { rendered = resolve; }));
    f.send({ type: "pty.data", data: Buffer.from("screen").toString("base64") });
    expect(f.records.some(r => r.type === "pty.ack")).toBe(false);
    rendered(); await Promise.resolve(); await Promise.resolve();
    expect(f.records.at(-1)).toMatchObject({ type: "pty.ack", bytes: 6 });
    pty.kill();
  });
  it("chunks a large paste with one input chunk in flight and rejects an over-limit queue", async () => {
    const f = fixture(), pty = await f.pending;
    pty.write("x".repeat(200_000));
    expect(f.records.filter((r) => r.type === "pty.input")).toHaveLength(1);
    for (let index = 0; index < 7; index++) f.send({ type: "pty.input-ready" });
    const input = f.records.filter((r) => r.type === "pty.input");
    expect(input).toHaveLength(7);
    expect(Buffer.concat(input.map((r) => Buffer.from(String(r.data), "base64"))).length).toBe(200_000);
    expect(() => pty.write(Buffer.alloc(8 * 1024 * 1024 + 1))).toThrow(/backpressure/);
    pty.kill();
  });
  it("discards queued input on disconnect, preserves Unicode and acknowledges output bytes", async () => {
    const f = fixture(), pty = await f.pending;
    const received: string[] = []; pty.onData((data) => { received.push(data); });
    const bytes = Buffer.from("한글");
    f.send({ type: "pty.data", data: bytes.subarray(0, 1).toString("base64") });
    f.send({ type: "pty.data", data: bytes.subarray(1).toString("base64") });
    expect(received.join("")).toBe("한글");
    expect(f.records.filter((r) => r.type === "pty.ack").map((r) => r.bytes)).toEqual([1, 5]);
    pty.write(Buffer.alloc(100_000)); f.send({ type: "disconnected" });
    const count = f.records.length; f.send({ type: "pty.input-ready" });
    expect(f.records).toHaveLength(count); expect(() => pty.write("again")).toThrow(/not replayed/);
  });
  it("separates detach from explicit close and preserves daemon identity", async () => {
    const f = fixture(), pty = await f.pending;
    expect(pty.sessionId).toBe("daemon-issued"); expect(pty.pid).toBe(1234);
    pty.resize(110, 40); expect(f.records.at(-1)).toMatchObject({ type: "pty.resize", cols: 110, rows: 40 });
    pty.kill(); expect(f.records.at(-1)?.type).toBe("pty.detach");
    expect(f.records.some((r) => r.type === "pty.close")).toBe(false);
    const second = fixture(), other = await second.pending; other.closeSession();
    expect(second.records.at(-1)?.type).toBe("pty.close"); second.send({ type: "pty.exit", exitCode: 0 });
  });
});
