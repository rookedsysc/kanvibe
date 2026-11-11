/** @vitest-environment node */
import { connect, createServer, type Socket } from "net";
import { mkdtemp, rm, stat } from "fs/promises";
import { tmpdir } from "os";
import path from "path";
import { once } from "events";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createHerdrClientProxy } from "@/lib/herdrClientProxy";

const cleanups: (() => Promise<void>)[] = [];
afterEach(async () => {
  vi.useRealTimers();
  for (const cleanup of cleanups.reverse()) await cleanup();
  cleanups.length = 0;
});

function stringBytes(text: string): Buffer {
  const bytes = Buffer.from(text);
  const length = bytes.length < 251 ? Buffer.from([bytes.length]) : Buffer.from([251, bytes.length & 255, bytes.length >> 8]);
  return Buffer.concat([length, bytes]);
}

function frame(payload: Buffer): Buffer {
  const length = Buffer.alloc(4);
  length.writeUInt32LE(payload.length);
  return Buffer.concat([length, payload]);
}

function paneInput(tag: 13 | 14, target: string): Buffer {
  return frame(Buffer.concat([Buffer.from([tag]), stringBytes(target), Buffer.from([1, 1]), stringBytes("must not reach initial terminal")]));
}

function snapshot(bootId = "boot-1", workspaceId = "w1", tabId = "w1:t1"): Buffer {
  return frame(Buffer.concat([Buffer.from([20]), stringBytes("shell.snapshot.v1"), stringBytes(JSON.stringify({
    boot_id: bootId, focused_workspace_id: workspaceId, focused_tab_id: tabId,
  }))]));
}

function readString(payload: Buffer, position: number): [string, number] {
  const marker = payload[position++];
  const length = marker === 251 ? payload.readUInt16LE(position) : marker;
  if (marker === 251) position += 2;
  return [payload.subarray(position, position + length).toString(), position + length];
}

function endpointRequest(payload: Buffer): { bootId: string; request: { id: string; method: string; params: Record<string, unknown> } } {
  expect(payload[0]).toBe(15);
  const [bootId, position] = readString(payload, 1);
  const [request] = readString(payload, position);
  return { bootId, request: JSON.parse(request) };
}

function response(bootId: string, id: string, body: unknown): Buffer {
  const json = JSON.stringify(body);
  return frame(Buffer.concat([Buffer.from([18]), stringBytes(bootId), stringBytes(id), Buffer.from([1]), stringBytes(json)]));
}

function frames(socket: Socket): { next: () => Promise<Buffer> } {
  let buffered: Buffer = Buffer.alloc(0);
  const ready: Buffer[] = [];
  const waiting: ((frame: Buffer) => void)[] = [];
  socket.on("data", (chunk: Buffer) => {
    buffered = Buffer.concat([buffered, chunk]);
    while (buffered.length >= 4 && buffered.length >= buffered.readUInt32LE(0) + 4) {
      const length = buffered.readUInt32LE(0);
      const payload = buffered.subarray(4, length + 4);
      buffered = buffered.subarray(length + 4);
      const resolve = waiting.shift();
      if (resolve) resolve(payload); else ready.push(payload);
    }
  });
  return { next: () => ready.length ? Promise.resolve(ready.shift()!) : new Promise((resolve) => waiting.push(resolve)) };
}

async function fixture() {
  const directory = await mkdtemp(path.join(tmpdir(), "kvhct-"));
  const upstreamPath = path.join(directory, "api");
  const server = createServer();
  const sockets = new Set<Socket>();
  server.on("connection", (socket) => { sockets.add(socket); socket.on("close", () => sockets.delete(socket)); });
  server.listen(upstreamPath);
  await once(server, "listening");
  cleanups.push(async () => {
    for (const socket of sockets) socket.destroy();
    await new Promise<void>((resolve) => server.close(() => resolve()));
    await rm(directory, { recursive: true, force: true });
  });
  const proxy = await createHerdrClientProxy(upstreamPath, "w1");
  cleanups.push(() => proxy.close());
  const upstreamConnected = once(server, "connection");
  const client = connect(proxy.socketPath);
  const received = frames(client);
  cleanups.push(async () => { client.destroy(); });
  const [upstream] = await upstreamConnected as [Socket];
  const sent = frames(upstream);
  return { proxy, client, upstream, received, sent, server };
}

async function initialize(setup: Awaited<ReturnType<typeof fixture>>) {
  setup.upstream.write(snapshot());
  const initial = endpointRequest(await setup.sent.next());
  setup.upstream.write(response(initial.bootId, initial.request.id, { result: { type: "ok" } }));
  await setup.received.next();
  return initial;
}

describe("Herdr 클라이언트 endpoint 프록시", () => {
  it("native 렌더링의 초기 snapshot과 surface는 보존하고 잘못된 초기 입력은 버린다", async () => {
    const setup = await fixture();
    const original = snapshot("boot-1", "w2", "w2:t1");
    const surface = frame(Buffer.from([13, 0]));
    setup.upstream.write(original);
    setup.upstream.write(surface);
    const initial = endpointRequest(await setup.sent.next());
    await new Promise<void>((resolve) => setup.client.write(paneInput(13, "w2:p1"), () => setImmediate(resolve)));
    const selected = snapshot("boot-1", "w1", "w1:t1");
    setup.upstream.write(response(initial.bootId, initial.request.id, { result: { type: "ok" } }));
    setup.upstream.write(selected);
    expect(frame(await setup.received.next())).toEqual(original);
    expect(frame(await setup.received.next())).toEqual(surface);
    expect(frame(await setup.received.next())).toEqual(selected);
    const currentInput = frame(Buffer.from([18, 1]));
    setup.client.write(currentInput);
    expect(frame(await setup.sent.next())).toEqual(currentInput);
  });

  it("대상 snapshot을 받아도 초기 focus 응답 전에는 실제 pane와 popup 입력을 버린다", async () => {
    const setup = await fixture();
    setup.upstream.write(snapshot());
    const initial = endpointRequest(await setup.sent.next());
    await new Promise<void>((resolve) => setup.client.write(paneInput(13, "w1:p1"), () => setImmediate(resolve)));
    await new Promise<void>((resolve) => setup.client.write(paneInput(14, "popup-terminal"), () => setImmediate(resolve)));
    setup.upstream.write(response(initial.bootId, initial.request.id, { result: { type: "ok" } }));
    await setup.received.next();
    const currentInput = frame(Buffer.from([18, 1]));
    setup.client.write(currentInput);
    expect(frame(await setup.sent.next())).toEqual(currentInput);
  });
  it("첫 snapshot의 boot ID로 해당 연결의 workspace만 선택한다", async () => {
    const setup = await fixture();
    const initial = await initialize(setup);
    expect(initial).toMatchObject({ bootId: "boot-1", request: { method: "workspace.focus", params: { workspace_id: "w1" } } });
    const changed = snapshot("boot-1", "w1", "w1:t3");
    setup.upstream.write(changed);
    expect(frame(await setup.received.next())).toEqual(changed);
    expect(setup.proxy.getActiveTabId()).toBe("w1:t3");
  });

  it("조각난 프레임과 native 응답을 원문 그대로 중계하고 자체 응답만 숨긴다", async () => {
    const setup = await fixture();
    const bytes = snapshot();
    setup.upstream.write(bytes.subarray(0, 2));
    setup.upstream.write(bytes.subarray(2, 9));
    setup.upstream.write(bytes.subarray(9));
    const initial = endpointRequest(await setup.sent.next());
    setup.upstream.write(response(initial.bootId, initial.request.id, { result: { type: "ok" } }));
    expect(frame(await setup.received.next())).toEqual(bytes);
    const native = response("boot-1", "native-request", { result: { type: "ok" } });
    setup.upstream.write(native);
    expect(frame(await setup.received.next())).toEqual(native);
    const input = frame(Buffer.from([18, 1]));
    setup.client.write(input);
    expect(frame(await setup.sent.next())).toEqual(input);
  });

  it("tab focus 요청의 성공 응답을 기다리고 서버 오류를 전달한다", async () => {
    const setup = await fixture();
    await initialize(setup);
    const selecting = setup.proxy.request("tab.focus", { tab_id: "w1:t2" });
    const sent = endpointRequest(await setup.sent.next());
    expect(sent.request).toMatchObject({ method: "tab.focus", params: { tab_id: "w1:t2" } });
    setup.upstream.write(response(sent.bootId, sent.request.id, { result: { type: "ok" } }));
    await expect(selecting).resolves.toBeUndefined();
    const missing = setup.proxy.request("tab.focus", { tab_id: "w1:missing" });
    const missingRequest = endpointRequest(await setup.sent.next());
    setup.upstream.write(response(missingRequest.bootId, missingRequest.request.id, { error: { code: "not_found", message: "tab missing" } }));
    await expect(missing).rejects.toThrow("tab missing");
  });

  it("handshake 도중 들어온 요청과 입력은 최초 Space 선택 뒤에 전송한다", async () => {
    const setup = await fixture();
    const selecting = setup.proxy.request("tab.focus", { tab_id: "w1:t2" });
    setup.upstream.write(snapshot());
    const initial = endpointRequest(await setup.sent.next());
    const input = frame(Buffer.from([18, 1]));
    setup.client.write(input);
    setup.upstream.write(response(initial.bootId, initial.request.id, { result: { type: "ok" } }));
    await setup.received.next();
    const forwarded = [await setup.sent.next(), await setup.sent.next()];
    const afterFocus = endpointRequest(forwarded.find((payload) => payload[0] === 15)!);
    expect(afterFocus.request.method).toBe("tab.focus");
    setup.upstream.write(response(afterFocus.bootId, afterFocus.request.id, { result: { type: "ok" } }));
    await selecting;
    expect(frame(forwarded.find((payload) => payload[0] === 18)!)).toEqual(input);
  });

  it("서버 boot ID가 바뀌면 초기 Space 선택을 다시 수행한다", async () => {
    const setup = await fixture();
    await initialize(setup);
    setup.upstream.write(snapshot("boot-2"));
    const initial = endpointRequest(await setup.sent.next());
    expect(initial.bootId).toBe("boot-2");
    expect(initial.request.method).toBe("workspace.focus");
    setup.upstream.write(response(initial.bootId, initial.request.id, { result: { type: "ok" } }));
    await setup.received.next();
  });

  it("초기 Space 선택 도중 서버가 바뀌어도 새 연결 초기화를 완료한다", async () => {
    const setup = await fixture();
    setup.upstream.write(snapshot("boot-old"));
    await setup.sent.next();
    await setup.received.next();
    setup.upstream.write(snapshot("boot-new"));
    const initial = endpointRequest(await setup.sent.next());
    expect(initial.bootId).toBe("boot-new");
    setup.upstream.write(response(initial.bootId, initial.request.id, { result: { type: "ok" } }));
    await setup.received.next();
    const selecting = setup.proxy.request("tab.focus", { tab_id: "w1:t2" });
    const sent = endpointRequest(await setup.sent.next());
    setup.upstream.write(response(sent.bootId, sent.request.id, { result: { type: "ok" } }));
    await expect(selecting).resolves.toBeUndefined();
  });

  it("이전 boot에서 보류한 입력을 새 서버 연결에 재전송하지 않는다", async () => {
    const setup = await fixture();
    setup.upstream.write(snapshot("boot-old", "w2", "w2:t1"));
    await setup.sent.next();
    const oldInput = frame(Buffer.from([18, 1]));
    await new Promise<void>((resolve) => setup.client.write(oldInput, () => setImmediate(resolve)));
    setup.upstream.write(snapshot("boot-new", "w2", "w2:t1"));
    const initial = endpointRequest(await setup.sent.next());
    setup.upstream.write(response(initial.bootId, initial.request.id, { result: { type: "ok" } }));
    setup.upstream.write(snapshot("boot-new"));
    await setup.received.next();
    const currentInput = frame(Buffer.from([18, 0]));
    setup.client.write(currentInput);
    expect(frame(await setup.sent.next())).toEqual(currentInput);
  });

  it("endpoint 명령의 요청 크기 상한을 넘으면 전송하지 않고 실패한다", async () => {
    const setup = await fixture();
    await initialize(setup);
    await expect(setup.proxy.request("tab.focus", { tab_id: "x".repeat(1024 * 1024) })).rejects.toThrow("요청 크기 초과");
  });

  it("허용 크기를 넘는 프레임은 연결을 닫고 대기 요청을 실패시킨다", async () => {
    const setup = await fixture();
    await initialize(setup);
    const pending = setup.proxy.request("tab.focus", { tab_id: "w1:t2" });
    const rejection = expect(pending).rejects.toThrow();
    await setup.sent.next();
    const closed = once(setup.client, "close");
    const prefix = Buffer.alloc(4);
    prefix.writeUInt32LE(32 * 1024 * 1024 + 1);
    setup.upstream.write(prefix);
    await closed;
    await rejection;
  });

  it("응답이 없는 endpoint 요청은 정해진 시간 후 실패한다", async () => {
    const setup = await fixture();
    await initialize(setup);
    vi.useFakeTimers();
    const pending = setup.proxy.request("tab.focus", { tab_id: "w1:t2" });
    const rejected = expect(pending).rejects.toThrow("시간 초과");
    await setup.sent.next();
    await vi.advanceTimersByTimeAsync(5_000);
    await rejected;
  });

  it("다시 연결한 TUI에는 같은 서버라도 Space 선택을 새로 보낸다", async () => {
    const setup = await fixture();
    await initialize(setup);
    setup.client.destroy();
    const connected = once(setup.server, "connection");
    const client = connect(setup.proxy.socketPath);
    cleanups.push(async () => { client.destroy(); });
    const [upstream] = await connected as [Socket];
    const sent = frames(upstream);
    upstream.write(snapshot());
    const initial = endpointRequest(await sent.next());
    expect(initial).toMatchObject({ bootId: "boot-1", request: { method: "workspace.focus", params: { workspace_id: "w1" } } });
    upstream.write(response(initial.bootId, initial.request.id, { result: { type: "ok" } }));
  });

  it("종료 시 연결과 소켓 디렉터리를 제거하고 반복 종료를 허용한다", async () => {
    const setup = await fixture();
    await initialize(setup);
    const socketPath = setup.proxy.socketPath;
    expect((await stat(socketPath)).mode & 0o777).toBe(0o600);
    await setup.proxy.close();
    await setup.proxy.close();
    await expect(stat(path.dirname(socketPath))).rejects.toMatchObject({ code: "ENOENT" });
    await expect(setup.proxy.request("tab.focus", { tab_id: "w1:t2" })).rejects.toThrow();
  });
});
