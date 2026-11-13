import { mkdtemp, mkdir, open, rm, writeFile } from "node:fs/promises";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { tmpdir } from "node:os";
import path from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { getAiSessionDetail } from "../aggregateAiSessions";
import { readAiSessionAttachment } from "../readSessionAttachment";
import { extractMessageAttachments } from "../messageAttachments";

const PNG = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aRZkAAAAASUVORK5CYII=";
let fixture: string;
const context = { worktreePath: "/qa/image-worktree", repoPath: "/qa/image-worktree" };

async function rollout(provider: "claude" | "codex", events: unknown[]) {
  const file = path.join(fixture, provider, "images.jsonl");
  await mkdir(path.dirname(file), { recursive: true });
  await writeFile(file, events.map(event => JSON.stringify(event)).join("\n"));
  return file;
}

beforeEach(async () => { fixture = await mkdtemp(path.join(tmpdir(), "kanvibe-image-history-")); });
afterEach(async () => { vi.unstubAllEnvs(); await rm(fixture, { recursive: true, force: true }); });

describe("image-aware Claude and Codex history", () => {
  it("preserves Claude image-only messages and returns lazy metadata alongside mixed text", async () => {
    const source = await rollout("claude", [
      { type: "user", sessionId: "image-session", cwd: context.worktreePath, timestamp: "2026-11-13T01:00:00Z", message: { role: "user", content: [{ type: "image", source: { type: "base64", media_type: "image/png", data: PNG } }] } },
      { type: "user", sessionId: "image-session", cwd: context.worktreePath, timestamp: "2026-11-13T01:00:01Z", message: { role: "user", content: [{ type: "text", text: "Inspect this" }, { type: "image", source: { type: "base64", media_type: "image/png", data: PNG } }] } },
    ]);
    const detail = await getAiSessionDetail(context, "claude", "image-session", source);
    expect(detail?.messages).toHaveLength(2);
    expect(detail?.messages.map(message => message.fullText)).toEqual(["Inspect this", ""]);
    expect(detail?.messages[1].attachments).toEqual([expect.objectContaining({ id: expect.any(String), kind: "image", mimeType: "image/png" })]);
    expect(JSON.stringify(detail)).not.toContain(PNG);
  });

  it("preserves distinct Codex image-only rows and their media identities across pagination", async () => {
    const source = await rollout("codex", [
      { type: "session_meta", payload: { id: "image-session", cwd: context.worktreePath } },
      { type: "response_item", timestamp: "2026-11-13T01:00:00Z", payload: { type: "message", role: "user", content: [{ type: "input_image", image_url: `data:image/png;base64,${PNG}` }] } },
      { type: "response_item", timestamp: "2026-11-13T01:00:00Z", payload: { type: "message", role: "user", content: [{ type: "local_image", path: "/qa/second.png" }] } },
    ]);
    const first = await getAiSessionDetail(context, "codex", "image-session", source, null, 1);
    const second = await getAiSessionDetail(context, "codex", "image-session", source, first?.nextCursor, 1);
    expect(first?.messages).toHaveLength(1);
    expect(second?.messages).toHaveLength(1);
    expect(first?.messages[0].attachments?.[0].id).not.toEqual(second?.messages[0].attachments?.[0].id);
    expect(second?.messages[0].attachments?.[0].mimeType).toBe("image/png");
    expect(JSON.stringify(second)).not.toContain(PNG);
  });

  it("does not expose an image from a foreign worktree or session", async () => {
    const source = await rollout("codex", [
      { type: "session_meta", payload: { id: "image-session", cwd: "/qa/foreign-worktree" } },
      { type: "response_item", payload: { type: "message", role: "user", content: [{ type: "input_image", image_url: `data:image/png;base64,${PNG}` }] } },
    ]);
    expect(await getAiSessionDetail(context, "codex", "image-session", source)).toBeNull();
    expect(await getAiSessionDetail(context, "codex", "other-session", source)).toBeNull();
  });

  it("renders actual Codex native image framing as an attachment and a clean caption", async () => {
    const source = await rollout("codex", [
      { type: "session_meta", payload: { id: "image-session", cwd: context.worktreePath } },
      { type: "response_item", payload: { type: "message", role: "user", content: [
        { type: "input_text", text: '<image name=[Image #1] path="/qa/owned image.png">' },
        { type: "input_image", image_url: `data:image/png;base64,${PNG}`, detail: "high" },
        { type: "input_text", text: "</image>" },
        { type: "input_text", text: "[Image #1] Inspect this" },
      ] } },
    ]);
    const detail = await getAiSessionDetail(context, "codex", "image-session", source);
    expect(detail?.messages[0].fullText).toBe("Inspect this");
    expect(detail?.messages[0].attachments).toHaveLength(1);
  });

  it("loads authorized inline bytes without a CLI and refuses an ID from another session", async () => {
    const source = await rollout("claude", [
      { type: "user", sessionId: "image-session", cwd: context.worktreePath, message: { role: "user", content: [{ type: "image", source: { type: "base64", media_type: "image/png", data: PNG } }] } },
    ]);
    const detail = await getAiSessionDetail(context, "claude", "image-session", source);
    const id = detail!.messages[0].attachments![0].id;
    expect(await readAiSessionAttachment(context, "claude", "image-session", id, source)).toEqual({ id, name: "image.png", mimeType: "image/png", dataUrl: `data:image/png;base64,${PNG}` });
    expect(await readAiSessionAttachment(context, "claude", "other-session", id, source)).toBeNull();
    expect(await readAiSessionAttachment({ worktreePath: "/other", repoPath: "/other" }, "claude", "image-session", id, source)).toBeNull();
    expect(await readAiSessionAttachment(context, "claude", "image-session", "/etc/passwd", source)).toBeNull();
  });

  it("loads only a recorded local image and gives a missing state after its removal", async () => {
    const imagePath = path.join(fixture, "owned image.png");
    await writeFile(imagePath, Buffer.from(PNG, "base64"));
    const source = await rollout("codex", [
      { type: "session_meta", payload: { id: "image-session", cwd: context.worktreePath } },
      { type: "response_item", payload: { type: "message", role: "user", content: [{ type: "local_image", path: imagePath }] } },
    ]);
    const detail = await getAiSessionDetail(context, "codex", "image-session", source);
    const id = detail!.messages[0].attachments![0].id;
    expect((await readAiSessionAttachment(context, "codex", "image-session", id, source))?.dataUrl).toBe(`data:image/png;base64,${PNG}`);
    await rm(imagePath);
    expect(await readAiSessionAttachment(context, "codex", "image-session", id, source)).toBeNull();
  });

  it("does not fetch remote URLs or return unsafe image markup", async () => {
    const source = await rollout("codex", [
      { type: "session_meta", payload: { id: "image-session", cwd: context.worktreePath } },
      { type: "response_item", timestamp: "2026-11-13T01:00:00Z", payload: { type: "message", role: "user", content: [{ type: "input_image", image_url: "https://example.invalid/private.png" }] } },
      { type: "response_item", timestamp: "2026-11-13T01:00:01Z", payload: { type: "message", role: "user", content: [{ type: "input_image", image_url: "data:image/svg+xml;base64,PHN2Zy8+" }] } },
    ]);
    const fetch = vi.fn();
    vi.stubGlobal("fetch", fetch);
    try {
      const detail = await getAiSessionDetail(context, "codex", "image-session", source);
      for (const message of detail!.messages) await expect(readAiSessionAttachment(context, "codex", "image-session", message.attachments![0].id, source)).rejects.toThrow();
      expect(fetch).not.toHaveBeenCalled();
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("does not inherit a foreign Claude cwd for later rows that omit cwd", async () => {
    const content = [{ type: "file", path: path.join(fixture, "owned-secret.txt") }];
    await writeFile(content[0].path, "Owned foreign task bytes");
    const source = await rollout("claude", [
      { type: "user", sessionId: "foreign-session", cwd: "/qa/foreign-worktree", message: { role: "user", content: "Foreign task" } },
      { type: "user", sessionId: "foreign-session", message: { role: "user", content } },
    ]);
    const id = extractMessageAttachments(content, source)[0].id;
    expect(await readAiSessionAttachment(context, "claude", "foreign-session", id, source)).toBeNull();
    expect(await getAiSessionDetail(context, "claude", "foreign-session", source)).toBeNull();
  });

  it("rejects a recorded FIFO without waiting for a writer", async () => {
    const fifo = path.join(fixture, "owned-fifo.png");
    await promisify(execFile)("mkfifo", [fifo]);
    const source = await rollout("codex", [
      { type: "session_meta", payload: { id: "image-session", cwd: context.worktreePath } },
      { type: "response_item", payload: { type: "message", role: "user", content: [{ type: "local_image", path: fifo }] } },
    ]);
    const detail = await getAiSessionDetail(context, "codex", "image-session", source);
    let settled = false;
    const result = readAiSessionAttachment(context, "codex", "image-session", detail!.messages[0].attachments![0].id, source).then(() => { settled = true; return "resolved"; }, () => { settled = true; return "rejected"; });
    let timeout: ReturnType<typeof setTimeout>;
    try {
      expect(await Promise.race([result, new Promise<string>(resolve => { timeout = setTimeout(() => resolve("blocked"), 1000); })])).toBe("rejected");
    } finally {
      clearTimeout(timeout!);
      if (!settled) { const writer = await open(fifo, "w"); await writer.write("owned probe release"); await writer.close(); await result; }
    }
  });

  it("cleans actual Claude image-paste markers while keeping literal user text", async () => {
    const source = await rollout("claude", [
      { type: "user", sessionId: "image-session", cwd: context.worktreePath, timestamp: "2026-11-13T01:00:00Z", imagePasteIds: [1], message: { role: "user", content: [{ type: "text", text: "[Image #1]Caption" }, { type: "image", source: { type: "base64", media_type: "image/png", data: PNG } }] } },
      { type: "user", sessionId: "image-session", cwd: context.worktreePath, timestamp: "2026-11-13T01:00:01Z", message: { role: "user", content: "[Image #1] literal text" } },
    ]);
    expect((await getAiSessionDetail(context, "claude", "image-session", source))?.messages.map(message => message.fullText)).toEqual(["[Image #1] literal text", "Caption"]);
  });
});
