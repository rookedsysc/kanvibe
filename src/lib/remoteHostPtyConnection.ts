import { randomUUID } from "node:crypto";
import { StringDecoder } from "node:string_decoder";
import { decodeHostBytes, HOST_BUFFER_BYTES, HOST_CHUNK_BYTES, type HostRecord } from "./remoteHostProtocol";

export interface RemotePtyOptions { key: string; command: string; cols: number; rows: number; sessionId?: string }
export interface RemotePtySession { sessionId: string; key: string; pid: number }
export interface RemoteHostPty {
  readonly sessionId: string;
  readonly pid: number;
  cols: number;
  rows: number;
  readonly process: string;
  write(data: string | Buffer): void;
  resize(cols: number, rows: number): void;
  kill(): void;
  closeSession(): void;
  onData(handler: (data: string) => void | Promise<void>): { dispose(): void };
  onExit(handler: (event: { exitCode: number; signal?: number; detached?: boolean }) => void): { dispose(): void };
}

/** attachment 하나를 terminal.ts에 연결한다. 단절 시 대기 입력을 버리고 호출자가 명시적으로 재attach한다. */
export function openRemoteHostPty(
  send: (record: HostRecord) => boolean,
  subscribe: (id: string, receive: (record: HostRecord) => void) => () => void,
  options: RemotePtyOptions,
): Promise<RemoteHostPty> {
  return new Promise((resolve, reject) => {
    const id = randomUUID();
    let ended = false, ready = false, sessionId = "", pid = 0;
    let cols = options.cols, rows = options.rows;
    const dataHandlers = new Set<(data: string) => void | Promise<void>>();
    const exitHandlers = new Set<(event: { exitCode: number; signal?: number; detached?: boolean }) => void>();
    const decoder = new StringDecoder("utf8");
    const input: Buffer[] = [], output: Array<{ data: string; bytes: number }> = [];
    let inputBytes = 0, outputBytes = 0, waitingInput = false, rendering = false;
    let pendingExit: { exitCode: number; signal?: number } | undefined;
    let lastExit: { exitCode: number; signal?: number; detached?: boolean } | undefined;
    let unsubscribe = () => {};
    const timer = setTimeout(() => finish({ exitCode: 255, detached: true }, new Error("PTY attach timed out; session outcome may be unknown")), 15_000);
    const finish = (event: { exitCode: number; signal?: number; detached?: boolean }, error?: Error) => {
      if (ended) return;
      ended = true; clearTimeout(timer); unsubscribe(); input.length = 0; inputBytes = 0; output.length = 0; outputBytes = 0;
      lastExit = event;
      if (!ready) reject(error ?? new Error("PTY detached before ready"));
      for (const handler of exitHandlers) handler(event);
    };
    const transmit = () => {
      if (ended || waitingInput || !input.length) return;
      waitingInput = true;
      try { send({ type: "pty.input", id, data: input[0].toString("base64") }); }
      catch (error) { finish({ exitCode: 255, detached: true }, error as Error); }
    };
    const detach = () => {
      if (ended) return;
      try { send({ type: "pty.detach", id }); } finally { finish({ exitCode: 0, detached: true }); }
    };
    const drainOutput = () => {
      if (ended || rendering) return;
      if (!output.length) { if (pendingExit) finish(pendingExit); return; }
      if (!dataHandlers.size) return;
      rendering = true;
      const chunk = output[0];
      const failed = () => { try { detach(); } catch { /* detach가 이미 종료 상태를 알렸다. */ } };
      const consumed = () => {
        if (ended) return;
        output.shift(); outputBytes -= chunk.bytes; rendering = false;
        try { send({ type: "pty.ack", id, bytes: chunk.bytes }); }
        catch { finish({ exitCode: 255, detached: true }); return; }
        drainOutput();
      };
      try {
        const pending = [...dataHandlers].map(handler => handler(chunk.data)).filter((result): result is Promise<void> => result !== undefined);
        if (pending.length) void Promise.all(pending).then(consumed, failed);
        else consumed();
      } catch { failed(); }
    };
    const terminal: RemoteHostPty = {
      get sessionId() { return sessionId; }, get pid() { return pid; }, get cols() { return cols; }, set cols(value) { cols = value; },
      get rows() { return rows; }, set rows(value) { rows = value; }, process: "remote-shell",
      write(data) {
        if (ended || pendingExit) throw new Error("PTY detached or exited; input not replayed");
        const bytes = Buffer.from(data);
        if (inputBytes + bytes.length > HOST_BUFFER_BYTES) throw new Error("PTY input backpressure limit");
        inputBytes += bytes.length;
        for (let offset = 0; offset < bytes.length; offset += HOST_CHUNK_BYTES) input.push(bytes.subarray(offset, offset + HOST_CHUNK_BYTES));
        transmit();
      },
      resize(nextCols, nextRows) { if (!ended) { send({ type: "pty.resize", id, cols: nextCols, rows: nextRows }); cols = nextCols; rows = nextRows; } },
      kill: detach,
      closeSession() { if (!ended) send({ type: "pty.close", id }); },
      onData(handler) {
        dataHandlers.add(handler); drainOutput();
        return { dispose: () => dataHandlers.delete(handler) };
      },
      onExit(handler) {
        exitHandlers.add(handler);
        if (lastExit) { const event = lastExit; queueMicrotask(() => { if (exitHandlers.has(handler)) handler(event); }); }
        return { dispose: () => exitHandlers.delete(handler) };
      },
    };
    unsubscribe = subscribe(id, (record) => {
      if (ended) return;
      if (record.type === "pty.ready") {
        if (typeof record.sessionId !== "string" || !Number.isSafeInteger(record.pid) || Number(record.pid) < 1) { finish({ exitCode: 255, detached: true }, new Error("Invalid PTY identity")); return; }
        sessionId = record.sessionId; pid = Number(record.pid); cols = Number(record.cols); rows = Number(record.rows);
        ready = true; clearTimeout(timer); resolve(terminal);
      } else if (record.type === "pty.data") {
        const bytes = decodeHostBytes(record.data), data = decoder.write(bytes);
        outputBytes += bytes.length;
        if (outputBytes > HOST_BUFFER_BYTES) { detach(); return; }
        output.push({ data, bytes: bytes.length }); drainOutput();
      } else if (record.type === "pty.input-ready") {
        if (waitingInput) { inputBytes -= input.shift()!.length; waitingInput = false; transmit(); }
      } else if (record.type === "pty.exit") {
        pendingExit = { exitCode: Number(record.exitCode), ...(typeof record.signal === "number" ? { signal: record.signal } : {}) };
        input.length = 0; inputBytes = 0; waitingInput = false; drainOutput();
      }
      else if (record.type === "pty.error" || record.type === "pty.detached" || record.type === "disconnected") finish({ exitCode: 255, detached: true }, new Error(String(record.error ?? "Host transport disconnected; input outcome may be unknown")));
    });
    try { send({ type: "pty.open", id, ...options }); }
    catch (error) { finish({ exitCode: 255, detached: true }, error as Error); }
  });
}
