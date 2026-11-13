import { randomUUID } from "node:crypto";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import { ensureKanvibeDataDirectory } from "@/lib/databasePaths";
import { decodeAttachmentDataUrl } from "@/lib/attachmentContent";
import { execGit } from "@/lib/gitOperations";
import { transferFileToRemoteHost } from "@/lib/remoteImagePaste";
import { parseSSHConfig } from "@/lib/sshConfig";
import { quoteForPosixShell } from "@/lib/worktree";
import { MAX_PANE_CHAT_ATTACHMENTS, type PaneChatAttachment, type PaneChatAttachmentInput } from "@/desktop/shared/paneChat";

export interface PaneChatAttachmentOwner {
  taskId: string;
  targetToken: string;
  identity: string;
  sshHost: string | null;
}

interface StoredAttachment {
  owner: PaneChatAttachmentOwner;
  attachment: PaneChatAttachment;
  path: string;
  directory: string | null;
  retained: boolean;
}

const staged = new Map<string, StoredAttachment>();
const pending = new Map<string, number>();
const IMAGE_EXTENSIONS: Record<string, string> = { "image/png": "png", "image/jpeg": "jpg", "image/gif": "gif", "image/webp": "webp" };
const ATTACHMENT_ID = /^[0-9a-f]{8}-[0-9a-f-]{27}$/i;

function attachmentError(message: string): Error {
  return Object.assign(new Error(message), { name: "PaneChatInputError" });
}
function ownerKey(owner: PaneChatAttachmentOwner): string {
  return JSON.stringify([owner.taskId, owner.targetToken, owner.identity, owner.sshHost]);
}

export async function storePaneChatAttachment(owner: PaneChatAttachmentOwner, input: PaneChatAttachmentInput): Promise<PaneChatAttachment> {
  if (!input || typeof input.name !== "string" || !input.name.trim() || input.name.length > 255 || /[\x00-\x1f\x7f]/.test(input.name) || typeof input.mimeType !== "string") throw attachmentError("첨부 파일 이름·형식이 올바르지 않습니다.");
  let content: ReturnType<typeof decodeAttachmentDataUrl>;
  try { content = decodeAttachmentDataUrl(input.dataUrl, input.mimeType); }
  catch (error) { throw attachmentError(error instanceof Error ? error.message : "첨부 파일을 읽을 수 없습니다."); }
  const key = ownerKey(owner);
  const count = [...staged.values()].filter((entry) => ownerKey(entry.owner) === key && !entry.retained).length;
  if (count + (pending.get(key) ?? 0) >= MAX_PANE_CHAT_ATTACHMENTS) throw attachmentError("첨부 파일은 한 번에 10개까지 추가할 수 있습니다.");
  pending.set(key, (pending.get(key) ?? 0) + 1);
  let directory: string | null = null;
  try {
    const id = randomUUID();
    const name = path.posix.basename(input.name.replaceAll("\\", "/"));
    const imageExtension = IMAGE_EXTENSIONS[content.mimeType];
    const extension = imageExtension ?? (path.extname(name).slice(1).toLowerCase().replace(/[^a-z0-9]/g, "").slice(0, 12) || "bin");
    let hostPath: string;
    if (owner.sshHost) {
      const config = (await parseSSHConfig()).find((entry) => entry.host === owner.sshHost);
      if (!config) throw attachmentError("첨부 파일을 받을 SSH 호스트를 찾을 수 없습니다.");
      hostPath = await transferFileToRemoteHost(config, content.buffer, extension);
    } else {
      const root = path.join(ensureKanvibeDataDirectory(), "pane-chat-attachments");
      await mkdir(root, { recursive: true, mode: 0o700 });
      directory = await mkdtemp(path.join(root, "draft-"));
      hostPath = path.join(directory, `${id}.${extension}`);
      await writeFile(hostPath, content.buffer, { mode: 0o600 });
    }
    const attachment: PaneChatAttachment = { id, name, mimeType: content.mimeType, kind: imageExtension ? "image" : "file", sizeBytes: content.buffer.length };
    staged.set(id, { owner: { ...owner }, attachment, path: hostPath, directory, retained: false });
    return attachment;
  } catch (error) {
    if (directory) await rm(directory, { recursive: true, force: true });
    throw error;
  } finally {
    const remaining = (pending.get(key) ?? 1) - 1;
    if (remaining) pending.set(key, remaining); else pending.delete(key);
  }
}

export function resolvePaneChatAttachments(owner: PaneChatAttachmentOwner, ids: string[]): { id: string; path: string; kind: "image" | "file" }[] {
  if (!Array.isArray(ids) || ids.length > MAX_PANE_CHAT_ATTACHMENTS || new Set(ids).size !== ids.length) throw attachmentError("첨부 파일 목록이 올바르지 않습니다.");
  return ids.map((id) => {
    const entry = typeof id === "string" && ATTACHMENT_ID.test(id) ? staged.get(id) : null;
    if (!entry || ownerKey(entry.owner) !== ownerKey(owner)) throw attachmentError("현재 CLI에 속하지 않거나 만료된 첨부 파일입니다.");
    return { id, path: entry.path, kind: entry.attachment.kind };
  });
}

export function retainPaneChatAttachments(owner: PaneChatAttachmentOwner, ids: string[]): void {
  for (const entry of resolvePaneChatAttachments(owner, ids)) staged.get(entry.id)!.retained = true;
}

export async function discardPaneChatAttachment(taskId: string, targetToken: string, id: string): Promise<void> {
  if (typeof id !== "string" || !ATTACHMENT_ID.test(id)) throw attachmentError("첨부 파일 식별자가 올바르지 않습니다.");
  const entry = staged.get(id);
  if (!entry) return;
  if (entry.owner.taskId !== taskId || entry.owner.targetToken !== targetToken) throw attachmentError("현재 CLI에 속하지 않는 첨부 파일입니다.");
  if (!entry.retained) {
    if (entry.owner.sshHost) await execGit(`rm -f -- ${quoteForPosixShell(entry.path)}`, entry.owner.sshHost, { timeoutMs: 5_000 });
    else if (entry.directory) await rm(entry.directory, { recursive: true, force: true });
  }
  staged.delete(id);
}
