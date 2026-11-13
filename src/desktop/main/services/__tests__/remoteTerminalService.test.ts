import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { once } from "node:events";
import { WebSocket } from "ws";
import https from "node:https";
import { execFileSync } from "node:child_process";

const state = vi.hoisted(() => ({ commands: [] as string[][], cwd: "", control: "", geometryPath: "", delayGeometry: false, blockGeometry: false, gracefulOwnerStop: false, unsupported: false, pending: false, children: [] as import("node:child_process").ChildProcess[] }));
vi.mock("node:child_process", async (original) => {
  const actual = await original<typeof import("node:child_process")>();
  return { ...actual, spawn: (file: string, args: string[], options: object) => {
    if (file !== "ssh") return actual.spawn(file, args, options);
    state.commands.push(args);
    const command = state.pending ? "exec sleep 20" : state.unsupported ? `printf '%s\\n' '{"type":"unsupported"}'` : args.at(-1)!;
    const child = actual.spawn("sh", ["-lc", command], { ...options, cwd: state.cwd });
    // Killing an SSH client delivers EOF to the remote process; keep that distinction in this local adapter.
    if (state.gracefulOwnerStop && !state.pending && !state.unsupported) child.kill = () => { child.stdin?.end(); return true; };
    state.children.push(child); return child;
  } };
});
vi.mock("@/lib/sshConfig", async (original) => ({
  ...await original<typeof import("@/lib/sshConfig")>(),
  parseSSHConfig: async () => [{ host: "fixture", hostname: "127.0.0.1", port: 22, username: "fixture", privateKeyPath: "/fixture-key" }],
}));
vi.mock("@/lib/herdr", async (original) => ({
  ...await original<typeof import("@/lib/herdr")>(),
  buildHerdrCommand: (_session: string, args: string[]) => {
    const paneId = args.at(-1)!;
    if (state.blockGeometry) {
      const blocked = `require('fs').writeFileSync(${JSON.stringify(state.geometryPath + ".pid")},String(process.pid));setInterval(()=>{},1000);`;
      return `node -e '${blocked.replace(/'/g, `'"'"'`)}'`;
    }
    const value = args[1] === "layout"
      ? `{layout:{area:{width:g.cols},focused_pane_id:${JSON.stringify(paneId)},zoomed:false,panes:[{pane_id:${JSON.stringify(paneId)},rect:{width:g.cols}}]}}`
      : `{pane:{pane_id:${JSON.stringify(paneId)},scroll:{viewport_rows:g.rows}}}`;
    const program = `const g=JSON.parse(require('fs').readFileSync(${JSON.stringify(state.geometryPath)},'utf8'));require('fs').writeFileSync(${JSON.stringify(state.geometryPath + '.read')},'read');setTimeout(()=>process.stdout.write(JSON.stringify({result:${value}})),${state.delayGeometry ? 200 : 0});`;
    return `node -e '${program.replace(/'/g, `'"'"'`)}'`;
  },
}));
vi.mock("@/lib/paneMirror", () => ({
  buildHerdrControlPaneCommand: (_session: string, _pane: string, cols: number, rows: number) => `${state.control} ${cols} ${rows}`,
  buildHerdrRecentScreenCommand: () => "printf 'history\\n'",
}));

let directory: string;
let service: typeof import("@/desktop/main/services/remoteTerminalService");
const clients: WebSocket[] = [];
const input = { sshHost: "fixture", deviceId: "device-a", taskId: "task-a", sessionName: "fixture-session", paneId: "w1:p1", cols: 80, rows: 24 };

beforeAll(() => { execFileSync(process.execPath, [path.resolve("scripts/build-remote-terminal-agent.cjs")], { stdio: "pipe" }); });
beforeEach(async () => {
  vi.resetModules();
  directory = await mkdtemp(path.join(os.tmpdir(), "kanvibe-agent-contract-"));
  state.cwd = directory; state.commands = []; state.unsupported = false; state.pending = false; state.children = []; state.delayGeometry = false; state.blockGeometry = false; state.gracefulOwnerStop = true;
  state.geometryPath = path.join(directory, "native-geometry");
  await writeFile(state.geometryPath, JSON.stringify({ cols: 120, rows: 40 }));
  const program = `const fs=require('fs'),readline=require('readline');fs.appendFileSync(${JSON.stringify(path.join(directory, "starts"))},'start\\n');fs.writeFileSync(${JSON.stringify(path.join(directory, "controller-pid"))},String(process.pid));let cols=Number(process.argv[1]||80),rows=Number(process.argv[2]||24);const storeSize=()=>fs.writeFileSync(${JSON.stringify(state.geometryPath)},JSON.stringify({cols,rows}));storeSize();const frame=(s,full=false)=>process.stdout.write(JSON.stringify({type:'terminal.frame',bytes:Buffer.from(s).toString('base64'),full,width:cols,height:rows})+'\\n');frame('initial:'+cols+'x'+rows,true);readline.createInterface({input:process.stdin}).on('line',l=>{const r=JSON.parse(l);fs.appendFileSync(${JSON.stringify(path.join(directory, "controller-records"))},l+'\\n');if(r.type==='terminal.input')frame(Buffer.from(r.bytes,'base64'));if(r.type==='terminal.resize'){cols=r.cols;rows=r.rows;storeSize();frame('resized:'+cols+'x'+rows,true);}if(r.type==='terminal.release')process.exit(0)})`;
  state.control = `node -e '${program.replace(/'/g, `'"'"'`)}'`;
  service = await import("@/desktop/main/services/remoteTerminalService");
});
afterEach(async () => {
  for (const client of clients.splice(0)) client.terminate();
  const exits = state.children.map((child) => child.exitCode !== null || child.signalCode !== null ? Promise.resolve() : once(child, "close"));
  service?.stopRemoteTerminalAgents();
  for (const child of state.children) child.kill("SIGTERM");
  await Promise.all(exits);
  await rm(directory, { recursive: true, force: true });
});

describe("bundled remote terminal provider over the exact SSH stdio bootstrap", () => {
  it("serves direct WSS and relay through one helper/controller with exact Unicode and NUL input", async () => {
    const [direct, relayed] = await Promise.all([
      service.prepareRemoteTerminalGrant(input),
      service.prepareRemoteTerminalGrant({ ...input, deviceId: "device-b" }),
    ]);
    expect(direct).not.toBeNull(); expect(relayed).not.toBeNull();
    expect(state.commands).toHaveLength(1);
    expect(state.commands[0]).toContain("StrictHostKeyChecking=yes");
    expect(state.commands[0].join(" ")).not.toContain(direct!.direct.token);
    const messages: string[] = [];
    const ws = new WebSocket(`wss://127.0.0.1:${direct!.direct.port}${direct!.direct.path}`, { rejectUnauthorized: false, headers: { authorization: `Bearer ${direct!.direct.token}` } });
    clients.push(ws); ws.on("message", (bytes) => messages.push(bytes.toString()));
    await once(ws, "open");
    await vi.waitFor(() => expect(messages.join("")).toContain("history\r\ninitial"));
    const text = "한글\0🌊";
    for (let index = 0; index < 100; index += 1) ws.send(text);
    await vi.waitFor(() => expect(messages.join("").split(text).length - 1).toBe(100));
    const paste = "x".repeat(1024 * 1024);
    ws.send(paste); ws.send("ENTER");
    await vi.waitFor(() => expect(messages.join("").includes(paste + "ENTER")).toBe(true));
    const relayMessages: string[] = [];
    const relayClosed = vi.fn();
    const relay = await service.attachRemoteTerminalRelay(relayed!.grantId, "device-b", "task-a", "w1:p1", (data) => relayMessages.push(data), relayClosed);
    relay.write("relay-only", false);
    await vi.waitFor(() => expect(relayMessages.join("")).toContain("relay-only"));
    expect(await readFile(path.join(directory, "starts"), "utf8")).toBe("start\n");
    expect(state.commands).toHaveLength(1);
    const closed = once(ws, "close");
    service.revokeRemoteTerminalDevice("device-a");
    expect((await closed)[0]).toBe(4001);
    relay.write("still-live", false);
    await vi.waitFor(() => expect(relayMessages.join("")).toContain("still-live"));
    expect(relayClosed).not.toHaveBeenCalled();
    relay.close();
  });

  it("keeps unused capabilities across abandoned direct handshakes so relay fallback works", async () => {
    const grant = (await service.prepareRemoteTerminalGrant(input))!;
    const ws = new WebSocket(`wss://127.0.0.1:${grant.direct.port}${grant.direct.path}`, { rejectUnauthorized: false, headers: { authorization: `Bearer ${grant.direct.token}` } });
    clients.push(ws);
    const first = once(ws, "message");
    await once(ws, "open"); await first;
    const closed = once(ws, "close"); ws.close(); await closed;
    const messages: string[] = [];
    const relay = await service.attachRemoteTerminalRelay(grant.grantId, "device-a", "task-a", "w1:p1", (text) => messages.push(text), () => {});
    relay.write("fallback", false);
    await vi.waitFor(() => expect(messages.join("")).toContain("fallback"));
    relay.close();
  });

  it("rejects wrong scoped relay and bad direct bearer before granting a websocket", async () => {
    const grant = (await service.prepareRemoteTerminalGrant(input))!;
    await expect(service.attachRemoteTerminalRelay(grant.grantId, "other-device", "task-a", "w1:p1", () => {}, () => {})).rejects.toThrow("scope");
    const ws = new WebSocket(`wss://127.0.0.1:${grant.direct.port}${grant.direct.path}`, { rejectUnauthorized: false, headers: { authorization: "Bearer wrong" } });
    clients.push(ws);
    const response = await new Promise<number>((resolve, reject) => { ws.once("unexpected-response", (_request, result) => { result.resume(); resolve(result.statusCode!); }); ws.once("error", reject); });
    expect(response).toBe(401);
    await expect(readFile(path.join(directory, "starts"))).rejects.toThrow();
  });

  it("shares an unsupported runtime result across descriptor and legacy fallback probes", async () => {
    state.unsupported = true;
    expect(await service.prepareRemoteTerminalGrant(input)).toBeNull();
    expect(await service.prepareRemoteTerminalGrant(input)).toBeNull();
    expect(state.commands).toHaveLength(1);
  });
  it("answers ordinary unauthenticated HTTPS requests instead of leaving sockets open", async () => {
    const grant = (await service.prepareRemoteTerminalGrant(input))!;
    const status = await new Promise<number>((resolve, reject) => {
      const request = https.get({ hostname: "127.0.0.1", port: grant.direct.port, path: "/", rejectUnauthorized: false }, (response) => { response.resume(); resolve(response.statusCode!); });
      request.setTimeout(500, () => request.destroy(new Error("Unauthenticated HTTP request hung")));
      request.once("error", reject);
    });
    expect(status).toBe(404);
  });

  it("stops a pending SSH bootstrap immediately when the owner stops", async () => {
    state.pending = true;
    const preparing = service.prepareRemoteTerminalGrant(input).catch((error: unknown) => error);
    await vi.waitFor(() => expect(state.commands).toHaveLength(1));
    service.stopRemoteTerminalAgents();
    const outcome = await Promise.race([preparing, new Promise((resolve) => setTimeout(() => resolve("still-pending"), 300))]);
    expect(outcome).toBeInstanceOf(Error);
  });

  it("B admission remains usable when A's clock is a day behind", async () => {
    const now = vi.spyOn(Date, "now").mockReturnValue(Date.now() - 24 * 60 * 60_000);
    try {
      const grant = (await service.prepareRemoteTerminalGrant(input))!;
      const ws = new WebSocket(`wss://127.0.0.1:${grant.direct.port}${grant.direct.path}`, { rejectUnauthorized: false, headers: { authorization: `Bearer ${grant.direct.token}` } });
      clients.push(ws);
      const first = once(ws, "message");
      await once(ws, "open");
      expect((await first)[0].toString()).toContain("history");
    } finally { now.mockRestore(); }
  });

  it("revokes one grant without closing another active stream belonging to the same task", async () => {
    const first = (await service.prepareRemoteTerminalGrant(input))!;
    const second = (await service.prepareRemoteTerminalGrant({ ...input, deviceId: "device-b" }))!;
    const firstClosed = vi.fn(); const secondClosed = vi.fn(); const data: string[] = [];
    const a = await service.attachRemoteTerminalRelay(first.grantId, "device-a", "task-a", "w1:p1", () => {}, firstClosed);
    const b = await service.attachRemoteTerminalRelay(second.grantId, "device-b", "task-a", "w1:p1", (text) => data.push(text), secondClosed);
    service.revokeRemoteTerminalGrant(first.grantId);
    await vi.waitFor(() => expect(firstClosed).toHaveBeenCalledWith(4000, "grant-revoked"));
    b.write("surviving-pane", false);
    await vi.waitFor(() => expect(data.join("")).toContain("surviving-pane"));
    expect(secondClosed).not.toHaveBeenCalled(); a.close(); b.close();
  });

  it("opens a freshly granted controller after the old controller exits on SIGTERM", async () => {
    const first = (await service.prepareRemoteTerminalGrant(input))!;
    const closed = vi.fn();
    await service.attachRemoteTerminalRelay(first.grantId, "device-a", "task-a", "w1:p1", () => {}, closed);
    const pid = Number(await readFile(path.join(directory, "controller-pid"), "utf8"));
    process.kill(pid, "SIGTERM");
    await vi.waitFor(() => expect(closed).toHaveBeenCalledWith(4000, "stream-failed"));
    const fresh = (await service.prepareRemoteTerminalGrant(input))!;
    const data: string[] = [];
    const relay = await service.attachRemoteTerminalRelay(fresh.grantId, "device-a", "task-a", "w1:p1", (text) => data.push(text), () => {});
    relay.write("fresh-controller", false);
    await vi.waitFor(() => expect(data.join("")).toContain("fresh-controller"));
    relay.close();
  });

  it("clamps only initial native geometry to the mobile viewport bound for a wide desktop pane", async () => {
    await writeFile(state.geometryPath, JSON.stringify({ cols: 2000, rows: 1000 }));
    const grant = (await service.prepareRemoteTerminalGrant({ ...input, cols: 2000, rows: 1000 }))!;
    const data: string[] = [];
    const ws = new WebSocket(`wss://127.0.0.1:${grant.direct.port}${grant.direct.path}`, { rejectUnauthorized: false, headers: { authorization: `Bearer ${grant.direct.token}` } });
    clients.push(ws); ws.on("message", (bytes) => data.push(bytes.toString()));
    await once(ws, "open");
    await vi.waitFor(() => expect(data.join("")).toContain("initial:500x500"));
    ws.send(Buffer.from(JSON.stringify({ type: "resize", cols: 80, rows: 24 })));
    await vi.waitFor(() => expect(data.join("")).toContain("resized:80x24"));
    const closed = once(ws, "close"); ws.close(); await closed;
    await vi.waitFor(async () => {
      const records = (await readFile(path.join(directory, "controller-records"), "utf8")).trim().split("\n").map((line) => JSON.parse(line));
      expect(records.slice(-2)).toEqual([{ type: "terminal.resize", cols: 2000, rows: 1000 }, { type: "terminal.release" }]);
    });
  });

  it("reads B's restored baseline when an unused ticket was issued during another viewport lease", async () => {
    const first = (await service.prepareRemoteTerminalGrant(input))!;
    const a = await service.attachRemoteTerminalRelay(first.grantId, "device-a", "task-a", "w1:p1", () => {}, () => {});
    expect(JSON.parse(await readFile(state.geometryPath, "utf8"))).toEqual({ cols: 80, rows: 24 });
    const delayed = (await service.prepareRemoteTerminalGrant({ ...input, deviceId: "device-b", cols: 80, rows: 24 }))!;
    a.close();
    await vi.waitFor(async () => expect(JSON.parse(await readFile(state.geometryPath, "utf8"))).toEqual({ cols: 120, rows: 40 }));
    const b = await service.attachRemoteTerminalRelay(delayed.grantId, "device-b", "task-a", "w1:p1", () => {}, () => {});
    expect(JSON.parse(await readFile(state.geometryPath, "utf8"))).toEqual({ cols: 80, rows: 24 });
    b.close();
    await vi.waitFor(async () => expect(JSON.parse(await readFile(state.geometryPath, "utf8"))).toEqual({ cols: 120, rows: 40 }));
  });

  it("does not spawn a controller after every client leaves during the asynchronous geometry read", async () => {
    state.delayGeometry = true;
    const grant = (await service.prepareRemoteTerminalGrant(input))!;
    const attaching = service.attachRemoteTerminalRelay(grant.grantId, "device-a", "task-a", "w1:p1", () => {}, () => {}).catch((error: unknown) => error);
    await vi.waitFor(async () => expect(await readFile(state.geometryPath + ".read", "utf8")).toBe("read"));
    service.revokeRemoteTerminalDevice("device-a");
    expect(await attaching).toBeInstanceOf(Error);
    await expect(readFile(path.join(directory, "starts"))).rejects.toThrow();
  });

  it("keeps another device's pending claim when the first requester is revoked during geometry lookup", async () => {
    state.delayGeometry = true;
    const a = (await service.prepareRemoteTerminalGrant(input))!;
    const b = (await service.prepareRemoteTerminalGrant({ ...input, deviceId: "device-b", cols: 90, rows: 30 }))!;
    const attachingA = service.attachRemoteTerminalRelay(a.grantId, "device-a", "task-a", "w1:p1", () => {}, () => {}).catch((error: unknown) => error);
    await vi.waitFor(async () => expect(await readFile(state.geometryPath + ".read", "utf8")).toBe("read"));
    const attachingB = service.attachRemoteTerminalRelay(b.grantId, "device-b", "task-a", "w1:p1", () => {}, () => {});
    service.revokeRemoteTerminalDevice("device-a");
    expect(await attachingA).toBeInstanceOf(Error);
    const relay = await attachingB;
    expect(await readFile(path.join(directory, "starts"), "utf8")).toBe("start\n");
    expect(JSON.parse(await readFile(state.geometryPath, "utf8"))).toEqual({ cols: 90, rows: 30 });
    relay.close();
    await vi.waitFor(async () => expect(JSON.parse(await readFile(state.geometryPath, "utf8"))).toEqual({ cols: 120, rows: 40 }));
  });

  it("cancels the entire owned geometry query process group when the SSH owner ends", async () => {
    state.blockGeometry = true; state.gracefulOwnerStop = true;
    let queryPid = 0;
    try {
      const grant = (await service.prepareRemoteTerminalGrant(input))!;
      const attaching = service.attachRemoteTerminalRelay(grant.grantId, "device-a", "task-a", "w1:p1", () => {}, () => {}).catch((error: unknown) => error);
      await vi.waitFor(async () => { queryPid = Number(await readFile(state.geometryPath + ".pid", "utf8")); expect(queryPid).toBeGreaterThan(0); });
      service.stopRemoteTerminalAgents();
      expect(await attaching).toBeInstanceOf(Error);
      await vi.waitFor(() => {
        let alive = false;
        try { process.kill(queryPid, 0); alive = true; } catch { /* The owned query has exited. */ }
        expect(alive).toBe(false);
      });
    } finally {
      for (const pid of [queryPid, ...state.children.map((child) => child.pid ?? 0)]) if (pid) {
        try { process.kill(pid, "SIGKILL"); } catch { /* Already reaped. */ }
      }
    }
  });

});
