import { randomUUID } from "node:crypto";
import { write } from "node:fs";
import { createRequire } from "node:module";
import path from "node:path";
import type { IPty } from "node-pty";
import { Terminal } from "@xterm/headless";
import { SerializeAddon } from "@xterm/addon-serialize";
import { createLocalShellEnvironment } from "./shellEnvironment";
import { decodeHostBytes, HOST_BUFFER_BYTES, HOST_CHUNK_BYTES, HOST_RECORD_BYTES, type HostRecord } from "./remoteHostProtocol";

interface PtyPeer { clientId: string; send(record: HostRecord): boolean }
interface Attachment { peer: PtyPeer; id: string; pendingOutput: number; ready: boolean }
interface InputChunk { attachment: Attachment; bytes: Buffer; offset: number }
interface PtySession {
  id: string;
  key: string;
  owner: string;
  pty: IPty;
  screen: Terminal;
  serializer: SerializeAddon;
  attachments: Set<Attachment>;
  input: InputChunk[];
  inputBytes: number;
  writing: boolean;
  renderingBytes: number;
  closing: boolean;
  exited: boolean;
  retry?: ReturnType<typeof setTimeout>;
}

const MAX_SESSIONS = 64;
const MAX_OWNER_SESSIONS = 16;
const OUTPUT_WINDOW_BYTES = 1024 * 1024;
const requireRuntime = createRequire(path.join(process.cwd(), "kanvibe-host-runtime.cjs"));

function dimensions(record: HostRecord): { cols: number; rows: number } {
  const cols = Number(record.cols), rows = Number(record.rows);
  if (!Number.isInteger(cols) || cols < 1 || cols > 1000 || !Number.isInteger(rows) || rows < 1 || rows > 500) throw new Error("Invalid PTY size");
  return { cols, rows };
}

/** SSH attachment와 독립적으로 원격 PTY를 소유한다. clientId 인증은 host server가 담당한다. */
export class RemoteHostPtyRegistry {
  private sessions = new Map<string, PtySession>();
  private attachments = new Map<PtyPeer, Map<string, { session: PtySession; attachment: Attachment }>>();
  private runtime?: typeof import("node-pty");

  constructor(private modulePath?: string) {}

  receive(peer: PtyPeer, record: HostRecord): void {
    const id = String(record.id);
    if (record.type === "pty.list") {
      const sessions = [...this.sessions.values()].filter((session) => session.owner === peer.clientId && !session.closing).map((session) => ({ sessionId: session.id, key: session.key, pid: session.pty.pid }));
      peer.send({ type: "pty.sessions", id, sessions }); return;
    }
    if (record.type === "pty.close" && record.sessionId !== undefined) {
      const session = this.sessions.get(String(record.sessionId));
      if (!session || session.owner !== peer.clientId) throw new Error("Unknown or unowned PTY session");
      session.pty.kill(); peer.send({ type: "pty.closed", id, sessionId: session.id }); return;
    }
    if (record.type === "pty.open") { this.open(peer, record, id); return; }
    const found = this.attachments.get(peer)?.get(id);
    if (!found) return;
    const { session, attachment } = found;
    switch (record.type) {
      case "pty.input": {
        const bytes = decodeHostBytes(record.data);
        if (session.inputBytes + bytes.length > HOST_BUFFER_BYTES) throw new Error("PTY input backpressure limit");
        session.inputBytes += bytes.length;
        session.input.push({ attachment, bytes, offset: 0 });
        this.writeInput(session); break;
      }
      case "pty.resize": {
        const { cols, rows } = dimensions(record);
        session.pty.resize(cols, rows); session.screen.resize(cols, rows); break;
      }
      case "pty.ack": {
        const bytes = Number(record.bytes);
        if (!Number.isSafeInteger(bytes) || bytes < 0 || bytes > attachment.pendingOutput) throw new Error("Invalid PTY output acknowledgement");
        attachment.pendingOutput -= bytes; break;
      }
      case "pty.detach": this.removeAttachment(session, attachment); break;
      case "pty.close": session.pty.kill(); break;
      default: throw new Error("Unknown PTY operation");
    }
  }

  detach(peer: PtyPeer): void {
    for (const { session, attachment } of this.attachments.get(peer)?.values() ?? []) this.removeAttachment(session, attachment);
    this.attachments.delete(peer);
  }

  close(): void {
    for (const session of this.sessions.values()) {
      session.exited = true; clearTimeout(session.retry); session.pty.kill(); session.screen.dispose();
    }
    this.sessions.clear(); this.attachments.clear();
  }

  private open(peer: PtyPeer, record: HostRecord, id: string): void {
    const { cols, rows } = dimensions(record);
    if (this.attachments.get(peer)?.has(id)) throw new Error("Duplicate PTY attachment id");
    if (typeof record.key !== "string" || !record.key.length || record.key.length > 2048 || typeof record.command !== "string" || !record.command.length || Buffer.byteLength(record.command) > HOST_RECORD_BYTES / 2) throw new Error("Invalid PTY key or command");
    let session = record.sessionId === undefined
      ? [...this.sessions.values()].find((entry) => entry.owner === peer.clientId && entry.key === record.key)
      : this.sessions.get(String(record.sessionId));
    if (record.sessionId !== undefined && (!session || session.owner !== peer.clientId)) throw new Error("Unknown or unowned PTY session");
    if (session?.closing || session?.exited) throw new Error("PTY session is closing or exited");
    if (!session) {
      if (this.sessions.size >= MAX_SESSIONS || [...this.sessions.values()].filter((entry) => entry.owner === peer.clientId).length >= MAX_OWNER_SESSIONS) throw new Error("PTY session capacity exceeded");
      this.runtime ??= requireRuntime(this.modulePath ?? "node-pty") as typeof import("node-pty");
      const pty = this.runtime.spawn("/bin/sh", ["-lc", record.command], {
        name: "xterm-256color", cols, rows, cwd: process.env.HOME || "/", env: createLocalShellEnvironment(),
      });
      const screen = new Terminal({ cols, rows, scrollback: 200, allowProposedApi: true });
      const serializer = new SerializeAddon(); screen.loadAddon(serializer);
      session = { id: randomUUID(), key: record.key, owner: peer.clientId, pty, screen, serializer, attachments: new Set(), input: [], inputBytes: 0, writing: false, renderingBytes: 0, closing: false, exited: false };
      this.sessions.set(session.id, session);
      const owned = session;
      pty.onData((data) => this.render(owned, data));
      pty.onExit(({ exitCode, signal }) => {
        if (owned.exited) return;
        owned.closing = true; clearTimeout(owned.retry);
        owned.screen.write("", () => {
          if (owned.exited) return;
          owned.exited = true; this.sessions.delete(owned.id);
          for (const attachment of [...owned.attachments]) {
            attachment.peer.send({ type: "pty.exit", id: attachment.id, sessionId: owned.id, exitCode, signal });
            this.removeAttachment(owned, attachment);
          }
          owned.screen.dispose();
        });
      });
    }
    if (session.attachments.size >= 16) throw new Error("PTY attachment capacity exceeded");
    const attachment: Attachment = { peer, id, pendingOutput: 0, ready: false };
    session.attachments.add(attachment);
    const peerAttachments = this.attachments.get(peer) ?? new Map();
    this.attachments.set(peer, peerAttachments); peerAttachments.set(id, { session, attachment });
    peer.send({ type: "pty.ready", id, sessionId: session.id, pid: session.pty.pid, cols: session.pty.cols, rows: session.pty.rows });
    const attached = session;
    // 제한된 현재 화면을 보낸 다음, 이후 렌더링 출력을 순서대로 구독한다.
    session.screen.write("", () => {
      if (attached.exited || this.attachments.get(peer)?.get(id)?.attachment !== attachment) return;
      this.output(attached, attachment, Buffer.from(attached.serializer.serialize()));
      if (this.attachments.get(peer)?.get(id)?.attachment === attachment) attachment.ready = true;
    });
  }

  private render(session: PtySession, data: string): void {
    if (session.exited) return;
    const bytes = Buffer.from(data);
    session.renderingBytes += bytes.length;
    if (session.renderingBytes > OUTPUT_WINDOW_BYTES) session.pty.pause();
    session.screen.write(data, () => {
      session.renderingBytes -= bytes.length;
      if (session.exited) return;
      for (const attachment of [...session.attachments]) if (attachment.ready) this.output(session, attachment, bytes);
      if (session.renderingBytes < OUTPUT_WINDOW_BYTES / 2) session.pty.resume();
    });
  }

  private output(session: PtySession, attachment: Attachment, bytes: Buffer): void {
    if (attachment.pendingOutput + bytes.length > OUTPUT_WINDOW_BYTES) {
      attachment.peer.send({ type: "pty.detached", id: attachment.id, error: "PTY output backpressure limit; reattach for current screen" });
      this.removeAttachment(session, attachment); return;
    }
    attachment.pendingOutput += bytes.length;
    for (let offset = 0; offset < bytes.length; offset += HOST_CHUNK_BYTES) attachment.peer.send({ type: "pty.data", id: attachment.id, data: bytes.subarray(offset, offset + HOST_CHUNK_BYTES).toString("base64") });
  }

  private removeAttachment(session: PtySession, attachment: Attachment): void {
    session.attachments.delete(attachment);
    if (this.attachments.get(attachment.peer)?.get(attachment.id)?.attachment === attachment) this.attachments.get(attachment.peer)?.delete(attachment.id);
    // detach 시 대기 입력을 버린다. 이미 시작한 커널 쓰기는 결과 불명이며 재전송하지 않는다.
    session.input = session.input.filter((chunk, index) => {
      if (chunk.attachment !== attachment || (session.writing && index === 0)) return true;
      session.inputBytes -= chunk.bytes.length; return false;
    });
  }

  private writeInput(session: PtySession): void {
    if (session.writing || session.exited || session.closing) return;
    const chunk = session.input[0]; if (!chunk) return;
    if (this.attachments.get(chunk.attachment.peer)?.get(chunk.attachment.id)?.attachment !== chunk.attachment) {
      session.input.shift(); session.inputBytes -= chunk.bytes.length; this.writeInput(session); return;
    }
    // 버전이 고정된 node-pty Unix runtime의 fd에 비동기로 쓴다.
    // public write()의 내부 큐 대신 커널 소비 완료를 확인해야 입력 ACK와 메모리 상한을 지킬 수 있다.
    const fd = (session.pty as IPty & { _fd: number })._fd;
    if (!Number.isInteger(fd) || fd < 0) throw new Error("Unsupported node-pty input runtime");
    session.writing = true;
    write(fd, chunk.bytes, chunk.offset, chunk.bytes.length - chunk.offset, null, (error, written) => {
      session.writing = false;
      if (session.exited || session.closing) return;
      if (error && error.code !== "EAGAIN" && error.code !== "EINTR") {
        chunk.attachment.peer.send({ type: "pty.detached", id: chunk.attachment.id, error: "PTY input failed; outcome may be unknown" });
        this.removeAttachment(session, chunk.attachment); return;
      }
      chunk.offset += written || 0;
      if (!error && chunk.offset >= chunk.bytes.length) {
        session.input.shift(); session.inputBytes -= chunk.bytes.length;
        if (this.attachments.get(chunk.attachment.peer)?.get(chunk.attachment.id)?.attachment === chunk.attachment) chunk.attachment.peer.send({ type: "pty.input-ready", id: chunk.attachment.id });
        this.writeInput(session);
      } else session.retry = setTimeout(() => this.writeInput(session), 2);
    });
  }
}
