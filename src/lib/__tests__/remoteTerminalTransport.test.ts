/** @vitest-environment node */
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { once } from "node:events";
import { PassThrough } from "node:stream";
import { Agent } from "node:https";
import { afterEach, beforeAll, beforeEach, describe, expect, it } from "vitest";
import WebSocket from "ws";
import selfsigned from "selfsigned";
import { startRemoteTerminalAgent } from "@/lib/remoteTerminalAgent";
import { quoteShellArgument } from "@/lib/hostFileAccess";
import type { RemoteTerminalWireGrant } from "@/lib/remoteTerminalProtocol";

let certificate: { cert: string; key: string };
let directory: string;
let owner: PassThrough;
let output: PassThrough;
let agent: Awaited<ReturnType<typeof startRemoteTerminalAgent>>;
let records: Array<Record<string, unknown>>;
let fixtureAgent: Agent;
const sockets = new Set<WebSocket>();

const poll = async (predicate: () => boolean | Promise<boolean>) => {
  const deadline = Date.now() + 4000;
  while (!await predicate()) {
    if (Date.now() >= deadline) throw new Error("Terminal transport condition timed out");
    await new Promise((resolve) => setTimeout(resolve, 5));
  }
};
const send = (record: object) => owner.write(`${JSON.stringify(record)}\n`);
let nextRequestId = 0;
async function request(method: string, body: object) {
  const id = String(++nextRequestId);
  send({ type: "request", id, method, ...body });
  await poll(() => records.some((record) => record.type === "response" && record.id === id));
  return records.find((record) => record.type === "response" && record.id === id)!;
}
const makeGrant = (id: string): RemoteTerminalWireGrant => ({
  grantId: id, token: `${id}-capability-${"a".repeat(64)}`, deviceId: id, taskId: "task-b", paneId: "w1:p1",
  targetKey: "owned-session:w1:p1", expiresAt: Date.now() + 60_000, cols: 80, rows: 24,
  geometryCommand: `printf '%s\\036%s' ${quoteShellArgument(JSON.stringify({ result: { pane: { pane_id: "w1:p1", scroll: { viewport_rows: 24 } } } }))} ${quoteShellArgument(JSON.stringify({ result: { layout: { area: { width: 80 }, zoomed: false, panes: [{ pane_id: "w1:p1", rect: { width: 80 } }] } } }))}`,
  controlCommand: `${quoteShellArgument(process.execPath)} ${quoteShellArgument(path.join(directory, "controller.cjs"))} ${quoteShellArgument(path.join(directory, "counter"))}`,
  snapshotCommand: "printf 'initial-history\\r\\n'",
});
const connect = (grant: RemoteTerminalWireGrant, token = grant.token) => {
  const ws = new WebSocket(`wss://127.0.0.1:${agent.port}/stream/${grant.grantId}`, {
    agent: fixtureAgent,
    headers: { authorization: `Bearer ${token}` },
  });
  sockets.add(ws);
  ws.once("close", () => sockets.delete(ws));
  return ws;
};

beforeAll(async () => {
  const generated = await selfsigned.generate([{ name: "commonName", value: "transport-fixture" }], { keySize: 2048 });
  certificate = { cert: generated.cert, key: generated.private };
});
beforeEach(async () => {
  fixtureAgent = new Agent({ ca: certificate.cert, checkServerIdentity: () => undefined });
  directory = await mkdtemp(path.join(tmpdir(), "kanvibe-terminal-transport-"));
  await writeFile(path.join(directory, "controller.cjs"), `
const fs = require('node:fs');
const readline = require('node:readline');
const counter = process.argv[2];
fs.appendFileSync(counter, 'start\\n');
function frame(text, full=false, width=80, height=24) {
  process.stdout.write(JSON.stringify({type:'terminal.frame',bytes:Buffer.from(text).toString('base64'),full,width,height})+'\\n');
}
frame('controller-ready',true);
readline.createInterface({input:process.stdin}).on('line', line=>{
  const message=JSON.parse(line);
  if(message.type==='terminal.input') {
    const input=Buffer.from(message.bytes,'base64').toString('utf8');
    fs.appendFileSync(counter, 'input:'+Buffer.from(input).toString('base64')+'\\n');
    frame(input);
  } else if(message.type==='terminal.resize') frame('viewport',true,message.cols,message.rows);
  else if(message.type==='terminal.release') process.exit(0);
});
process.stdin.on('end',()=>process.exit(0));
`);
  owner = new PassThrough(); output = new PassThrough(); records = [];
  let partial = "";
  output.on("data", (chunk: Buffer) => {
    partial += chunk.toString();
    let end;
    while ((end = partial.indexOf("\n")) >= 0) {
      records.push(JSON.parse(partial.slice(0, end))); partial = partial.slice(end + 1);
    }
  });
  agent = await startRemoteTerminalAgent(certificate, owner, output);
});
afterEach(async () => {
  for (const socket of sockets) socket.terminate();
  agent.stop(); owner.end(); output.end();
  fixtureAgent.destroy();
  await rm(directory, { recursive: true, force: true });
});

describe("B terminal real TLS/stdio contract", () => {
  it("direct와 A relay의 연속 입력이 같은 B controller 하나에 도착한다", async () => {
    const directGrant = makeGrant("direct-one");
    const relayGrant = makeGrant("relay-one");
    expect(await request("grant", { grant: directGrant })).toMatchObject({ ok: true });
    expect(await request("grant", { grant: relayGrant })).toMatchObject({ ok: true });
    const direct = connect(directGrant);
    const frames: string[] = [];
    direct.on("message", (data) => frames.push(data.toString()));
    await once(direct, "open");
    await poll(() => frames.some((frame) => frame.includes("controller-ready")));
    direct.send("DIRECT_INPUT");
    await poll(() => frames.some((frame) => frame.includes("DIRECT_INPUT")));
    expect(await request("attach", { relayId: "relay-transport", grantId: relayGrant.grantId, deviceId: relayGrant.deviceId, taskId: relayGrant.taskId, paneId: relayGrant.paneId })).toMatchObject({ ok: true });
    send({ type: "data", relayId: "relay-transport", binary: false, data: Buffer.from("RELAY_INPUT").toString("base64") });
    await poll(() => records.some((record) => record.type === "output" && typeof record.data === "string" && Buffer.from(record.data, "base64").toString().includes("RELAY_INPUT")));
    const counter = (await readFile(path.join(directory, "counter"), "utf8")).trim().split("\n");
    expect(counter).toEqual(["start", `input:${Buffer.from("DIRECT_INPUT").toString("base64")}`, `input:${Buffer.from("RELAY_INPUT").toString("base64")}`]);
  });

  it("잘못된 capability와 다른 pane relay를 거절하고 기기 해제는 직접 연결도 닫는다", async () => {
    const grant = makeGrant("device-one");
    await request("grant", { grant });
    const rejected = connect(grant, "wrong-capability");
    await expect(once(rejected, "open")).rejects.toThrow(/401/);
    expect(await request("attach", { relayId: "wrong-scope", grantId: grant.grantId, deviceId: grant.deviceId, taskId: grant.taskId, paneId: "w2:p1" })).toMatchObject({ ok: false });
    const direct = connect(grant);
    await once(direct, "open");
    const closed = once(direct, "close");
    expect(await request("revokeDevice", { deviceId: grant.deviceId })).toMatchObject({ ok: true });
    const [code, reason] = await closed;
    expect([code, reason.toString()]).toEqual([4001, "device-unpaired"]);
  });
});
