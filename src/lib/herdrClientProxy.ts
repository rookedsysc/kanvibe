import { connect, createServer, type Socket } from "net";
import { chmod, mkdtemp, rm } from "fs/promises";
import { tmpdir } from "os";
import path from "path";
import { randomUUID } from "crypto";

// Herdr endpoint generation 1의 고정 태그와 bincode 표준 프레이밍을 사용한다.
const ENDPOINT_CONTROL_TAG = 20;
const ENDPOINT_REQUEST_TAG = 15;
const ENDPOINT_RESPONSE_TAG = 18;
const PANE_INPUT_TAG = 13;
const POPUP_INPUT_TAG = 14;
const MAX_FRAME_BYTES = 32 * 1024 * 1024;
const MAX_ENDPOINT_REQUEST_BYTES = 1024 * 1024;
const REQUEST_TIMEOUT_MS = 5_000;

function encodeString(text: string): Buffer {
  const bytes = Buffer.from(text);
  if (bytes.length < 251) return Buffer.concat([Buffer.from([bytes.length]), bytes]);
  const prefix = Buffer.alloc(bytes.length <= 0xffff ? 3 : 5);
  prefix[0] = prefix.length === 3 ? 251 : 252;
  if (prefix.length === 3) prefix.writeUInt16LE(bytes.length, 1);
  else prefix.writeUInt32LE(bytes.length, 1);
  return Buffer.concat([prefix, bytes]);
}

function readString(payload: Buffer, position: number): [string, number] {
  const marker = payload[position++];
  let length = marker;
  if (marker >= 251) {
    const width = marker === 251 ? 2 : marker === 252 ? 4 : marker === 253 ? 8 : 0;
    if (!width || position + width > payload.length) throw new Error("Herdr 문자열 길이가 올바르지 않습니다.");
    const size = width === 2 ? BigInt(payload.readUInt16LE(position))
      : width === 4 ? BigInt(payload.readUInt32LE(position)) : payload.readBigUInt64LE(position);
    if (size > BigInt(MAX_FRAME_BYTES)) throw new Error("Herdr 문자열 크기 초과");
    length = Number(size);
    position += width;
  }
  if (length === undefined || position + length > payload.length) throw new Error("Herdr 문자열이 끊겼습니다.");
  return [payload.subarray(position, position + length).toString("utf8"), position + length];
}

function frame(payload: Buffer): Buffer {
  const prefix = Buffer.alloc(4);
  prefix.writeUInt32LE(payload.length);
  return Buffer.concat([prefix, payload]);
}

function readFrames(socket: Socket, receive: (payload: Buffer) => void, fail: (error: Error) => void): void {
  const header = Buffer.alloc(4);
  let headerBytes = 0;
  let payloadLength = 0;
  let payloadBytes = 0;
  let payloadChunks: Buffer[] = [];
  socket.on("data", (chunk: Buffer) => {
    try {
      let offset = 0;
      while (offset < chunk.length) {
        if (headerBytes < 4) {
          const count = Math.min(4 - headerBytes, chunk.length - offset);
          chunk.copy(header, headerBytes, offset, offset + count);
          headerBytes += count;
          offset += count;
          if (headerBytes < 4) break;
          payloadLength = header.readUInt32LE(0);
          if (!payloadLength || payloadLength > MAX_FRAME_BYTES) throw new Error("Herdr 프레임 크기가 올바르지 않습니다.");
        }
        const count = Math.min(payloadLength - payloadBytes, chunk.length - offset);
        if (count) payloadChunks.push(chunk.subarray(offset, offset + count));
        payloadBytes += count;
        offset += count;
        if (payloadBytes < payloadLength) break;
        const payload = payloadChunks.length === 1 ? payloadChunks[0] : Buffer.concat(payloadChunks, payloadLength);
        headerBytes = 0;
        payloadBytes = 0;
        payloadChunks = [];
        receive(payload);
      }
    } catch (error) {
      fail(error instanceof Error ? error : new Error(String(error)));
    }
  });
}

interface PendingRequest {
  method: string;
  params: Record<string, unknown>;
  bootId: string | null;
  resolve: () => void;
  reject: (error: Error) => void;
  timeout: ReturnType<typeof setTimeout>;
}

class HerdrEndpointConnection {
  private readonly upstream: Socket;
  private readonly requestPrefix = `kanvibe-proxy:${randomUUID()}:`;
  private readonly pending = new Map<string, PendingRequest>();
  private sequence = 0;
  private bootId: string | null = null;
  private initialized = false;
  private awaitingInitialWorkspace = true;
  private closed = false;
  private queuedInput: Buffer[] = [];
  private queuedInputBytes = 0;
  private activeTabId: string | null = null;

  constructor(private readonly client: Socket, upstreamSocketPath: string, private readonly workspaceId: string) {
    this.upstream = connect(upstreamSocketPath);
    const fail = (error: Error) => this.destroy(error);
    client.on("error", fail);
    this.upstream.on("error", fail);
    client.on("close", () => this.destroy());
    this.upstream.on("close", () => this.destroy());
    readFrames(client, (payload) => this.forwardInput(payload), fail);
    readFrames(this.upstream, (payload) => this.receive(payload), fail);
  }

  getActiveTabId(): string | null {
    return this.activeTabId;
  }

  request(method: string, params: Record<string, unknown>, initialize = false): Promise<void> {
    if (this.closed) return Promise.reject(new Error("Herdr 클라이언트 연결이 종료됐습니다."));
    return new Promise<void>((resolve, reject) => {
      const id = `${this.requestPrefix}${++this.sequence}`;
      const timeout = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error("Herdr 클라이언트 요청 시간 초과"));
      }, REQUEST_TIMEOUT_MS);
      const request: PendingRequest = { method, params, bootId: null, resolve, reject, timeout };
      this.pending.set(id, request);
      if (this.bootId && (this.initialized || initialize)) this.sendRequest(id, request);
    });
  }

  private sendRequest(id: string, request: PendingRequest): void {
    request.bootId = this.bootId;
    const json = JSON.stringify({ id, method: request.method, params: request.params });
    const payload = Buffer.concat([Buffer.from([ENDPOINT_REQUEST_TAG]), encodeString(this.bootId!), encodeString(json)]);
    if (payload.length > MAX_ENDPOINT_REQUEST_BYTES) {
      this.finishRequest(id, new Error("Herdr 클라이언트 요청 크기 초과"));
      return;
    }
    this.upstream.write(frame(payload));
  }

  private finishRequest(id: string, error?: Error): void {
    const request = this.pending.get(id);
    if (!request) return;
    clearTimeout(request.timeout);
    this.pending.delete(id);
    if (error) request.reject(error); else request.resolve();
  }

  private forwardInput(payload: Buffer): void {
    // 초기 화면의 pane ID가 다른 task를 가리킬 수 있어 그 입력은 재전송하지 않는다.
    if ((this.awaitingInitialWorkspace || !this.initialized)
      && (payload[0] === PANE_INPUT_TAG || payload[0] === POPUP_INPUT_TAG)) return;
    if (this.bootId && !this.initialized) {
      this.queuedInputBytes += payload.length + 4;
      if (this.queuedInputBytes > MAX_FRAME_BYTES) throw new Error("Herdr 초기 연결 입력 크기 초과");
      this.queuedInput.push(frame(payload));
      return;
    }
    this.writeThrough(this.client, this.upstream, frame(payload));
  }

  private writeThrough(source: Socket, destination: Socket, bytes: Buffer): void {
    if (!destination.write(bytes)) {
      source.pause();
      destination.once("drain", () => source.resume());
    }
  }

  private receive(payload: Buffer): void {
    if (payload[0] === ENDPOINT_RESPONSE_TAG && this.receiveResponse(payload)) return;
    if (payload[0] === ENDPOINT_CONTROL_TAG) {
      const [kind, position] = readString(payload, 1);
      if (kind === "shell.snapshot.v1") {
        const [json] = readString(payload, position);
        this.receiveSnapshot(JSON.parse(json));
      }
    }
    // 초기 surface를 버리면 Herdr의 후속 delta가 기준 프레임을 잃는다.
    this.writeThrough(this.upstream, this.client, frame(payload));
  }

  private receiveResponse(payload: Buffer): boolean {
    const [bootId, idPosition] = readString(payload, 1);
    const [id, finalPosition] = readString(payload, idPosition);
    if (!id.startsWith(this.requestPrefix)) return false;
    const request = this.pending.get(id);
    if (!request || request.bootId !== bootId) return true;
    if (payload[finalPosition] !== 1) {
      this.finishRequest(id, new Error("Herdr 클라이언트 응답이 단일 프레임이 아닙니다."));
      return true;
    }
    const [json] = readString(payload, finalPosition + 1);
    const response = JSON.parse(json);
    this.finishRequest(id, response.error ? new Error(response.error.message ?? response.error.code) :
      response.result ? undefined : new Error("Herdr 클라이언트 응답에 result가 없습니다."));
    return true;
  }

  private receiveSnapshot(snapshot: { boot_id?: unknown; focused_workspace_id?: unknown; focused_tab_id?: unknown }): void {
    if (typeof snapshot.boot_id !== "string" || !snapshot.boot_id) throw new Error("Herdr snapshot에 boot ID가 없습니다.");
    this.activeTabId = snapshot.focused_workspace_id === this.workspaceId && typeof snapshot.focused_tab_id === "string"
      ? snapshot.focused_tab_id : null;
    if (this.bootId !== snapshot.boot_id) {
      this.awaitingInitialWorkspace = true;
      this.queuedInput = [];
      this.queuedInputBytes = 0;
    }
    if (snapshot.focused_workspace_id === this.workspaceId) this.awaitingInitialWorkspace = false;
    if (this.bootId === snapshot.boot_id) return;
    for (const [id, request] of this.pending) {
      if (request.bootId) this.finishRequest(id, new Error("Herdr 서버 연결이 갱신됐습니다."));
    }
    this.bootId = snapshot.boot_id;
    const initializingBootId = this.bootId;
    this.initialized = false;
    void this.request("workspace.focus", { workspace_id: this.workspaceId }, true).then(() => {
      if (this.closed || this.bootId !== initializingBootId) return;
      this.initialized = true;
      for (const bytes of this.queuedInput) this.upstream.write(bytes);
      this.queuedInput = [];
      this.queuedInputBytes = 0;
      for (const [id, request] of this.pending) if (!request.bootId) this.sendRequest(id, request);
    }).catch((error: Error) => {
      if (this.bootId === initializingBootId) this.destroy(error);
    });
  }

  destroy(error = new Error("Herdr 클라이언트 연결이 종료됐습니다.")): void {
    if (this.closed) return;
    this.closed = true;
    this.activeTabId = null;
    for (const id of this.pending.keys()) this.finishRequest(id, error);
    this.queuedInput = [];
    this.client.destroy();
    this.upstream.destroy();
  }
}

export async function createHerdrClientProxy(upstreamSocketPath: string, workspaceId: string): Promise<{
  socketPath: string;
  request(method: string, params: Record<string, unknown>): Promise<void>;
  getActiveTabId(): string | null;
  close(): Promise<void>;
}> {
  const directory = await mkdtemp(path.join(tmpdir(), "kvhc-"));
  const socketPath = path.join(directory, "client");
  let connection: HerdrEndpointConnection | null = null;
  let closing: Promise<void> | null = null;
  const server = createServer((client) => {
    connection?.destroy();
    connection = new HerdrEndpointConnection(client, upstreamSocketPath, workspaceId);
  });
  try {
    await new Promise<void>((resolve, reject) => {
      server.once("error", reject);
      server.listen(socketPath, resolve);
    });
    await chmod(socketPath, 0o600);
  } catch (error) {
    server.close();
    await rm(directory, { recursive: true, force: true });
    throw error;
  }
  return {
    socketPath,
    request: (method, params) => connection?.request(method, params) ?? Promise.reject(new Error("Herdr 클라이언트가 아직 연결되지 않았습니다.")),
    getActiveTabId: () => connection?.getActiveTabId() ?? null,
    close: () => closing ??= (async () => {
      connection?.destroy();
      await new Promise<void>((resolve) => server.close(() => resolve()));
      await rm(directory, { recursive: true, force: true });
    })(),
  };
}
