// @vitest-environment node
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { chmod, lstat, mkdtemp, readFile, rm, symlink } from "node:fs/promises";
import { spawn } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import { builtinModules } from "node:module";
import { createServer, connect, type Socket } from "node:net";
import { request } from "node:http";
import path from "node:path";
import { homedir } from "node:os";
import { createHostRecordDecoder, encodeHostRecord, type HostRecord } from "../remoteHostProtocol";
import { startRemoteHostServer } from "../remoteHostServer";
import { HOST_BOOTSTRAP_LOADER } from "../remoteHostBootstrap";

describe("remote host provider", () => {
  let directory: string;
  let server: Awaited<ReturnType<typeof startRemoteHostServer>> | undefined;
  const clients: Socket[] = [];
  beforeEach(async () => { directory = await mkdtemp("/tmp/kvhp-"); });
  afterEach(async () => {
    clients.splice(0).forEach((client) => client.destroy());
    await server?.close(); server = undefined;
    await rm(directory, { recursive: true, force: true });
  });
  async function start() {
    server = await startRemoteHostServer({ namespace: "qa-provider", buildHash: "a".repeat(64), rootDirectory: directory });
    return server;
  }
  async function client(clientId = "client-one", hello: Record<string, unknown> = {}) {
    const socket = connect(server!.paths.rpcSocketPath); clients.push(socket);
    const queued: HostRecord[] = [];
    const waiters: Array<{ type: string; resolve: (record: HostRecord) => void }> = [];
    socket.on("data", createHostRecordDecoder((record) => {
      const index = waiters.findIndex((entry) => entry.type === record.type);
      if (index >= 0) waiters.splice(index, 1)[0].resolve(record); else queued.push(record);
    }));
    function next(type: string): Promise<HostRecord> {
      const index = queued.findIndex((record) => record.type === type);
      if (index >= 0) return Promise.resolve(queued.splice(index, 1)[0]);
      return new Promise((resolve) => waiters.push({ type, resolve }));
    }
    socket.write(encodeHostRecord({ type: "hello", version: 1, namespace: "qa-provider", clientId, role: "desktop", buildHash: "a".repeat(64), ...hello }));
    return { socket, next, send: (record: HostRecord) => socket.write(encodeHostRecord(record)) };
  }
  function post(clientId: string, body = "{}", headers: Record<string, string> = {}) {
    return new Promise<{ status: number; body: string }>((resolve, reject) => {
      const req = request({ socketPath: server!.paths.hookSocketPath, method: "POST", path: "/api/hooks/status", headers: { "x-kanvibe-client-id": clientId, ...headers } }, (response) => {
        let result = ""; response.on("data", (chunk) => { result += chunk; });
        response.on("end", () => resolve({ status: response.statusCode!, body: result }));
      }); req.on("error", reject); req.end(body);
    });
  }
  it("creates private sockets, keeps daemon identity across attachment disconnects", async () => {
    await start();
    const one = await client(); const ready = await one.next("ready"); one.socket.destroy();
    const two = await client("client-two"); const again = await two.next("ready");
    expect(again.instanceId).toBe(ready.instanceId);
    expect(again.pid).toBe(process.pid);
    expect((await lstat(server!.paths.rpcSocketPath)).mode & 0o777).toBe(0o600);
  });
  it("rejects a competing server without unlinking the live socket", async () => {
    await start();
    await expect(startRemoteHostServer({ namespace: "qa-provider", buildHash: "a".repeat(64), rootDirectory: directory })).rejects.toThrow();
    const peer = await client(); expect((await peer.next("ready")).instanceId).toBe(server!.instanceId);
  });
  it("rejects symlink and nonprivate roots", async () => {
    const link = `${directory}-link`; await symlink(directory, link);
    await expect(startRemoteHostServer({ namespace: "qa-provider", buildHash: "a".repeat(64), rootDirectory: link })).rejects.toThrow(/trusted|private|symlink/i);
    await rm(link); await chmod(directory, 0o755);
    await expect(startRemoteHostServer({ namespace: "qa-provider", buildHash: "a".repeat(64), rootDirectory: directory })).rejects.toThrow(/private/i);
  });
  it.each([{ role: "hook" }, { namespace: "prod" }, { version: 2 }, { buildHash: "b".repeat(64) }])("rejects mismatched hello %j", async (hello) => {
    await start(); const peer = await client("bad-client", hello);
    expect((await peer.next("error")).error).toBeTruthy();
  });
  it("executes concurrent streams, preserves stderr/exit and sends interactive stdin once", async () => {
    await start(); const peer = await client(); await peer.next("ready");
    peer.send({ type: "exec.open", id: "interactive", command: "read line; printf 'got:%s' \"$line\"; printf problem >&2; exit 7", timeoutMs: 3000, maxBufferBytes: 4096, interactive: true });
    peer.send({ type: "exec.stdin", id: "interactive", data: Buffer.from("hello\n").toString("base64"), end: true });
    const output = await peer.next("exec.data");
    expect(Buffer.from(String(output.data), "base64").toString()).toBe("got:hello");
    const stderr = await peer.next("exec.data"); expect(stderr.stream).toBe("stderr");
    expect((await peer.next("exec.exit")).code).toBe(7);
  });
  it("cancels only its selected request and leaves another command usable", async () => {
    await start(); const peer = await client(); await peer.next("ready");
    peer.send({ type: "exec.open", id: "slow", command: "sleep 30", timeoutMs: 35000, maxBufferBytes: 4096 });
    peer.send({ type: "exec.cancel", id: "slow" });
    expect((await peer.next("exec.exit")).id).toBe("slow");
    peer.send({ type: "exec.open", id: "alive", command: "printf alive", timeoutMs: 3000, maxBufferBytes: 4096 });
    expect(Buffer.from(String((await peer.next("exec.data")).data), "base64").toString()).toBe("alive");
    expect((await peer.next("exec.exit")).code).toBe(0);
  });
  it("forwards a real Unix socket without owning the upstream lifetime", async () => {
    await start(); const upstreamPath = path.join(directory, "herdr.sock");
    const upstream = createServer((socket) => socket.pipe(socket));
    await new Promise<void>((resolve) => upstream.listen(upstreamPath, resolve)); await chmod(upstreamPath, 0o600);
    try {
      const peer = await client(); await peer.next("ready");
      peer.send({ type: "socket.open", id: "native", path: upstreamPath }); await peer.next("socket.ready");
      peer.send({ type: "socket.data", id: "native", data: Buffer.from("native bytes").toString("base64") });
      expect((await peer.next("socket.data-ready")).id).toBe("native");
      expect(Buffer.from(String((await peer.next("socket.data")).data), "base64").toString()).toBe("native bytes");
      peer.send({ type: "socket.close", id: "native" }); expect((await peer.next("socket.close")).id).toBe("native");
      expect(upstream.listening).toBe(true);
    } finally { await new Promise<void>((resolve) => upstream.close(() => resolve())); }
  });
  it("routes live hooks to one desktop and durably accepts offline status events", async () => {
    await start(); expect((await post("offline", '{"taskId":"fixture","status":"done"}')).status).toBe(202);
    const peer = await client(); await peer.next("ready");
    peer.send({ type: "hook.subscribe", id: "subscribe" });
    const pending = post("client-one", '{"taskId":"fixture","status":"done"}'); const hook = await peer.next("hook.request");
    expect(hook.path).toBe("/api/hooks/status");
    expect(Buffer.from(String(hook.body), "base64").toString()).toBe('{"taskId":"fixture","status":"done"}');
    peer.send({ type: "hook.response", id: hook.id, status: 202, headers: { "content-type": "application/json" }, body: Buffer.from('{"ok":true}').toString("base64") });
    expect(await pending).toEqual({ status: 202, body: '{"ok":true}' });
    peer.socket.destroy();
    expect((await post("client-one", '{"taskId":"fixture","status":"done"}')).status).toBe(202);
  });
  it("rejects duplicate live client IDs without replacing the first desktop", async () => {
    await start(); const first = await client(); const original = await first.next("ready");
    const second = await client(); expect((await second.next("error")).error).toBeTruthy();
    first.send({ type: "status", id: "identity" }); expect((await first.next("status")).instanceId).toBe(original.instanceId);
  });
  it("strips server environment from user commands", async () => {
    const original = process.env.KANVIBE_PROVIDER_SECRET;
    process.env.KANVIBE_PROVIDER_SECRET = "private-runtime";
    try {
      await start(); const peer = await client(); await peer.next("ready");
      peer.send({ type: "exec.open", id: "env", command: "printf '<%s>' \"${KANVIBE_PROVIDER_SECRET-unset}\"", timeoutMs: 3000, maxBufferBytes: 4096 });
      expect(Buffer.from(String((await peer.next("exec.data")).data), "base64").toString()).toBe("<unset>");
      await peer.next("exec.exit");
    } finally { if (original === undefined) delete process.env.KANVIBE_PROVIDER_SECRET; else process.env.KANVIBE_PROVIDER_SECRET = original; }
  });
  it("acknowledges consumed stdin chunks so a bulk upload can stay bounded", async () => {
    await start(); const peer = await client(); await peer.next("ready");
    peer.send({ type: "exec.open", id: "upload", command: "wc -c", timeoutMs: 3000, maxBufferBytes: 4096, interactive: true });
    const chunk = Buffer.alloc(32768, "x");
    for (let index = 0; index < 257; index += 1) {
      peer.send({ type: "exec.stdin", id: "upload", data: chunk.toString("base64") });
      expect((await peer.next("exec.stdin-ready")).id).toBe("upload");
    }
    peer.send({ type: "exec.stdin", id: "upload", data: "", end: true });
    expect(Number(Buffer.from(String((await peer.next("exec.data")).data), "base64").toString().trim())).toBe(chunk.length * 257);
    expect((await peer.next("exec.exit")).code).toBe(0);
  });
  it("keeps unbounded streaming output alive while bounded commands enforce their limit", async () => {
    await start(); const peer = await client(); await peer.next("ready");
    const command = "printf '%65536s' x";
    peer.send({ type: "exec.open", id: "bounded", command, timeoutMs: 3000, maxBufferBytes: 1024 });
    expect((await peer.next("exec.exit")).error).toMatch(/maxBuffer/);
    peer.send({ type: "exec.open", id: "streaming", command, timeoutMs: 3000, maxBufferBytes: 0 });
    const result = await Promise.race([peer.next("exec.exit"), peer.next("error")]);
    expect(result.type).toBe("exec.exit"); expect(result.code).toBe(0);
  });
  it("rejects browser Origin at hook ingress before headers can be filtered", async () => {
    await start();
    const response = await post("offline", "{}", { Origin: "https://untrusted.example" });
    expect(response.status).toBe(403);
    expect(JSON.parse(response.body)).toEqual({ success: false, error: "브라우저 요청은 허용하지 않습니다" });
  });
  it("executes a real forty KiB file command without applying the chunk byte limit to command text", async () => {
    await start(); const peer = await client(); await peer.next("ready");
    const contents = Buffer.alloc(40 * 1024, "x").toString("base64");
    const filePath = path.join(directory, "large-file.txt");
    const command = `printf '%s' '${contents}' | base64 -d > '${filePath}'; wc -c < '${filePath}'`;
    peer.send({ type: "exec.open", id: "large-file", command, timeoutMs: 3000, maxBufferBytes: 4096 });
    const result = await Promise.race([peer.next("exec.data"), peer.next("error")]);
    expect(result.type).toBe("exec.data");
    expect(Number(Buffer.from(String(result.data), "base64").toString().trim())).toBe(40 * 1024);
    expect((await peer.next("exec.exit")).code).toBe(0);
  });
});

async function waitForHostFixtureExit(pid: number | undefined): Promise<void> {
  if (!pid) return;
  const deadline = Date.now() + 2000;
  while (true) {
    try { process.kill(pid, 0); }
    catch (error) { if ((error as NodeJS.ErrnoException).code === "ESRCH") return; throw error; }
    if (Date.now() >= deadline) throw new Error("Owned host fixture did not stop: " + pid);
    await new Promise(resolve => setTimeout(resolve, 10));
  }
}

describe("standalone host bootstrap", () => {
  it("publishes one complete immutable bundle for eight concurrent loaders", async () => {
    const namespace = `qa-r-${randomUUID().slice(0, 8)}`;
    const bundleDirectory = await mkdtemp("/tmp/kvhost-race-");
    const { build } = await import("vite");
    await build({ configFile: false, logLevel: "silent", resolve: { conditions: ["node", "import"], mainFields: ["main"] }, build: {
      target: "node18", outDir: bundleDirectory, emptyOutDir: true, minify: false,
      lib: { entry: path.resolve("src/lib/remoteHostServer.ts"), formats: ["cjs"], fileName: () => "host-server.cjs" },
      rollupOptions: { external: [...builtinModules, ...builtinModules.map((name) => `node:${name}`)] },
    } });
    const source = await readFile(path.join(bundleDirectory, "host-server.cjs"), "utf8");
    const hash = createHash("sha256").update(source).digest("hex");
    // 느린 파일시스템의 부분 쓰기를 실제 loader 프로세스에서 재현한다.
    const slowWrite = `const slowFs=require('node:fs'),write=slowFs.writeFileSync;slowFs.writeFileSync=(fd,text,...args)=>{if(typeof fd==='number'&&typeof text==='string'){const n=Math.floor(text.length/2);write(fd,text.slice(0,n),...args);Atomics.wait(new Int32Array(new SharedArrayBuffer(4)),0,0,300);write(fd,text.slice(n),...args);}else write(fd,text,...args);};`;
    const peers = Array.from({ length: 8 }, () => {
      const child = spawn(process.execPath, ["-e", slowWrite + HOST_BOOTSTRAP_LOADER], { stdio: ["pipe", "pipe", "pipe"] });
      let stderr = "", lines = Buffer.alloc(0), bridged = false;
      let bootstrapReady!: () => void, resolveReady!: (record: HostRecord) => void, rejectReady!: (error: Error) => void;
      const bootstrapped = new Promise<void>((resolve) => { bootstrapReady = resolve; });
      const ready = new Promise<HostRecord>((resolve, reject) => { resolveReady = resolve; rejectReady = reject; });
      const decode = createHostRecordDecoder((record) => { if (record.type === "ready") resolveReady(record); });
      child.stderr.on("data", (chunk) => { stderr += chunk; });
      child.on("close", (code) => rejectReady(new Error(`Loader ${code}: ${stderr}`)));
      child.stdout.on("data", (chunk: Buffer) => {
        if (bridged) { decode(chunk); return; }
        lines = Buffer.concat([lines, chunk]);
        let index: number;
        while ((index = lines.indexOf(10)) >= 0) {
          const record = JSON.parse(lines.subarray(0, index).toString()); lines = lines.subarray(index + 1);
          if (record.type === "bootstrap-ready") bootstrapReady();
          if (record.type === "bridge-ready") {
            bridged = true; child.stdin.write(encodeHostRecord({ type: "hello", version: 1, namespace, clientId: randomUUID(), role: "desktop", buildHash: hash }));
            if (lines.length) decode(lines); break;
          }
        }
      });
      return { child, bootstrapped, ready };
    });
    let daemonPid: number | undefined;
    try {
      await Promise.all(peers.map((peer) => peer.bootstrapped));
      peers.forEach((peer) => peer.child.stdin.write(JSON.stringify({ source, hash, namespace }) + "\n"));
      const results = await Promise.allSettled(peers.map((peer) => peer.ready));
      const successes = results.filter((result): result is PromiseFulfilledResult<HostRecord> => result.status === "fulfilled");
      daemonPid = successes[0]?.value.pid as number | undefined;
      expect(results.map((result) => result.status)).toEqual(Array(8).fill("fulfilled"));
      expect(new Set(successes.map((result) => result.value.instanceId)).size).toBe(1);
      expect(new Set(successes.map((result) => result.value.pid)).size).toBe(1);
    } finally {
      peers.forEach((peer) => { peer.child.stdin.end(); peer.child.kill(); });
      if (daemonPid) process.kill(daemonPid, "SIGTERM");
      await waitForHostFixtureExit(daemonPid);
      await rm(path.join(homedir(), ".local", "state", "kanvibe", "host", namespace), { recursive: true, force: true });
      await rm(`/tmp/kv-${process.getuid!()}/${namespace}`, { recursive: true, force: true });
      await rm(bundleDirectory, { recursive: true, force: true });
    }
  }, 15_000);
  it("retains a detached daemon across two binary bridges and stops only its owned fixture", async () => {
    const namespace = `qa-b-${randomUUID().slice(0, 8)}`;
    const bundleDirectory = await mkdtemp("/tmp/kvhost-bundle-");
    const { build } = await import("vite");
    await build({ configFile: false, logLevel: "silent", resolve: { conditions: ["node", "import"], mainFields: ["main"] }, build: {
      target: "node18", outDir: bundleDirectory, emptyOutDir: true, minify: false,
      lib: { entry: path.resolve("src/lib/remoteHostServer.ts"), formats: ["cjs"], fileName: () => "host-server.cjs" },
      rollupOptions: { external: [...builtinModules, ...builtinModules.map((name) => `node:${name}`)] },
    } });
    const source = await readFile(path.join(bundleDirectory, "host-server.cjs"), "utf8");
    const hash = createHash("sha256").update(source).digest("hex");
    const children: ReturnType<typeof spawn>[] = [];
    const started = async () => {
      const child = spawn(process.execPath, ["-e", HOST_BOOTSTRAP_LOADER], { stdio: ["pipe", "pipe", "pipe"] }); children.push(child);
      let stderr = ""; child.stderr!.on("data", (chunk) => { stderr += chunk; });
      const queue: HostRecord[] = []; const waiters: Array<{ type: string; resolve: (record: HostRecord) => void }> = [];
      const receive = (record: HostRecord) => {
        const index = waiters.findIndex((entry) => entry.type === record.type);
        if (index >= 0) waiters.splice(index, 1)[0].resolve(record); else queue.push(record);
      };
      const decode = createHostRecordDecoder(receive);
      let lines = Buffer.alloc(0), bridged = false;
      child.stdout!.on("data", (chunk: Buffer) => {
        if (bridged) { decode(chunk); return; }
        lines = Buffer.concat([lines, chunk]);
        let index: number;
        while ((index = lines.indexOf(10)) >= 0) {
          const record = JSON.parse(lines.subarray(0, index).toString()); lines = lines.subarray(index + 1);
          if (record.type === "bootstrap-ready") child.stdin!.write(JSON.stringify({ source, hash, namespace }) + "\n");
          if (record.type === "bridge-ready") {
            bridged = true; child.stdin!.write(encodeHostRecord({ type: "hello", version: 1, namespace, clientId: randomUUID(), role: "desktop", buildHash: hash }));
            if (lines.length) decode(lines); break;
          }
        }
      });
      const next = (type: string) => {
        const index = queue.findIndex((entry) => entry.type === type);
        if (index >= 0) return Promise.resolve(queue.splice(index, 1)[0]);
        return new Promise<HostRecord>((resolve, reject) => {
          const timeout = setTimeout(() => reject(new Error(`Bootstrap ${type} timed out: ${stderr}`)), 10_000);
          waiters.push({ type, resolve: (record) => { clearTimeout(timeout); resolve(record); } });
        });
      };
      return { child, next, send: (record: HostRecord) => child.stdin!.write(encodeHostRecord(record)) };
    };
    let stop: Awaited<ReturnType<typeof started>> | undefined;
    let daemonPid: number | undefined;
    try {
      const first = await started(); const one = await first.next("ready"); daemonPid = Number(one.pid);
      first.child.stdin!.end();
      const second = await started(); stop = second; const two = await second.next("ready");
      expect(two.instanceId).toBe(one.instanceId); expect(two.pid).toBe(one.pid);
      second.send({ type: "exec.open", id: "binary", command: "printf '\\377\\000a'", timeoutMs: 3000, maxBufferBytes: 4096 });
      const output = await second.next("exec.data");
      expect([...Buffer.from(String(output.data), "base64")]).toEqual([255, 0, 97]);
      expect((await second.next("exec.exit")).code).toBe(0);
    } finally {
      if (stop) { stop.send({ type: "stop", id: "owned-stop" }); await stop.next("stopped"); }
      children.forEach((child) => { child.stdin?.end(); child.kill(); });
      await waitForHostFixtureExit(daemonPid);
      await rm(path.join(homedir(), ".local", "state", "kanvibe", "host", namespace), { recursive: true, force: true });
      await rm(`/tmp/kv-${process.getuid!()}/${namespace}`, { recursive: true, force: true });
      await rm(bundleDirectory, { recursive: true, force: true });
    }
  }, 20_000);
});
