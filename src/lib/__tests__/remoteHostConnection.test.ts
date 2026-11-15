// @vitest-environment node
import { afterEach, describe, expect, it, vi } from "vitest";
import { PassThrough } from "node:stream";
import { EventEmitter } from "node:events";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import type { ExecFileOptions } from "node:child_process";
import { createHostRecordDecoder, encodeHostRecord, type HostRecord } from "@/lib/remoteHostProtocol";
import { connectHostProcess, type HostProcess } from "@/lib/remoteHostConnection";

const owned: Array<() => void> = [];
afterEach(() => { for (const close of owned.splice(0)) close(); });

function fixture() {
  const child = Object.assign(new EventEmitter(), {
    stdin: new PassThrough(), stdout: new PassThrough(), stderr: new PassThrough(),
    kill: () => { child.emit("close", 0); return true; },
  });
  const records: HostRecord[] = [];
  const decode = createHostRecordDecoder((r) => {
    records.push(r);
    if (r.type === "hello") child.stdout.write(encodeHostRecord({ type: "ready", version: 1, namespace: "qa-client", buildHash: "a", pid: 123, instanceId: "server", capabilities: ["exec", "socket", "hooks"] }));
  });
  let boot = "";
  child.stdin.on("data", (b: Buffer) => {
    if (boot === "done") return decode(b);
    boot += b.toString();
    if (boot.includes("\n")) { boot = "done"; child.stdout.write('{"type":"bridge-ready"}\n'); }
  });
  const ready = connectHostProcess(child as HostProcess, { source: "bundle", hash: "a", namespace: "qa-client", clientId: "client" });
  child.stdout.write('{"type":"bootstrap-ready"}\n');
  owned.push(() => child.kill());
  return { child, records, ready, send: (r: HostRecord) => child.stdout.write(encodeHostRecord(r)) };
}

describe("host connection", () => {
  it("correlates concurrent commands and preserves Unicode split across chunks", async () => {
    const f = fixture(); const c = await f.ready;
    const a = c!.execute("first"), b = c!.execute("second");
    const requests = f.records.filter((r) => r.type === "exec.open");
    const bytes = Buffer.from("한글\u0000");
    for (const part of [bytes.subarray(0, 1), bytes.subarray(1)]) f.send({ type: "exec.data", id: requests[0].id, stream: "stdout", data: part.toString("base64") });
    f.send({ type: "exec.exit", id: requests[1].id, code: 0, signal: null });
    f.send({ type: "exec.exit", id: requests[0].id, code: 0, signal: null });
    await expect(a).resolves.toBe("한글\u0000"); await expect(b).resolves.toBe("");
  });
  it("rejects an unknown command outcome on disconnect without replay", async () => {
    const f = fixture(); const c = await f.ready;
    const result = c!.execute("write-once");
    f.child.emit("close", 255);
    await expect(result).rejects.toThrow(/disconnected/i);
    expect(f.records.filter((r) => r.type === "exec.open")).toHaveLength(1);
  });
  it("marks completed remote errors and includes stdout/stderr", async () => {
    const f = fixture(); const c = await f.ready;
    const result = c!.execute("false"); const r = f.records.find((r) => r.type === "exec.open")!;
    f.send({ type: "exec.data", id: r.id, stream: "stderr", data: Buffer.from("denied").toString("base64") });
    f.send({ type: "exec.exit", id: r.id, code: 7, signal: null });
    await expect(result).rejects.toMatchObject({ code: 7, stderr: "denied", remoteCommandCompleted: true });
  });
});

describe("host endpoint identity from real ssh effective configuration", () => {
  let directory: string | undefined;
  afterEach(async () => {
    vi.doUnmock("node:child_process"); vi.resetModules();
    if (directory) await rm(directory, { recursive: true, force: true });
    directory = undefined;
  });

  async function endpointKeys(proxyCommand = "") {
    directory = await mkdtemp("/tmp/kvhost-identity-");
    const configFile = path.join(directory, "ssh-config");
    await writeFile(configFile, `Host route-a route-b\n  HostName 127.0.0.1\n  User fixture\n  ${proxyCommand}\n`, { mode: 0o600 });
    const observed: string[] = [];
    vi.resetModules();
    vi.doMock("node:child_process", async () => {
      const actual = await vi.importActual<typeof import("node:child_process")>("node:child_process");
      const boundary = vi.fn(actual.execFile);
      Object.defineProperty(boundary, Symbol.for("nodejs.util.promisify.custom"), {
        value: (file: string, args: string[], options: ExecFileOptions) => new Promise((resolve, reject) => {
          actual.execFile(file, ["-F", configFile, ...args], { ...options, encoding: "utf8" }, (error, stdout, stderr) => {
            if (error) { reject(error); return; }
            observed.push(stdout.split("\n").filter((line) => !line.startsWith("host ")).join("\n"));
            resolve({ stdout, stderr });
          });
        }),
      });
      return { ...actual, execFile: boundary };
    });
    const { getRemoteHostEndpointKey } = await import("@/lib/remoteHostConnection");
    const config = { hostname: "127.0.0.1", port: 22, username: "fixture", privateKeyPath: "/tmp/kvhost-identity-unused-key" };
    const keys = await Promise.all(["route-a", "route-b"].map((host) => getRemoteHostEndpointKey({ ...config, host })));
    expect(observed).toHaveLength(2);
    expect(observed[0]).toBe(observed[1]);
    return keys;
  }

  it("shares the key for ordinary aliases with identical effective configuration", async () => {
    const [one, two] = await endpointKeys();
    expect(one).toBe(two);
  });

  it("separates aliases when ProxyCommand retains the original-host %n token", async () => {
    const [one, two] = await endpointKeys("ProxyCommand ssh gateway nc %n 22");
    expect(one).not.toBe(two);
  });

  it("separates aliases when ProxyCommand retains the host-key-alias %k token", async () => {
    const [one, two] = await endpointKeys("ProxyCommand ssh gateway nc %k 22");
    expect(one).not.toBe(two);
  });
});
