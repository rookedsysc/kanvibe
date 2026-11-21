import { open } from "node:fs/promises";
import { constants } from "node:fs";
import { execGit } from "@/lib/gitOperations";
import { quoteShellArgument } from "@/lib/hostFileAccess";
import { decodeAttachmentDataUrl, detectImageMimeType, MAX_ATTACHMENT_BYTES } from "@/lib/attachmentContent";
import { getAiSessionDetail } from "./aggregateAiSessions";
import { findMessageAttachment } from "./messageAttachments";
import { determineMatchScope, getCachedOrParse, readJsonLines } from "./shared";
import type { AiSessionAttachmentContent, AiSessionProvider, AiSessionReaderContext } from "./types";

async function readRecordedFile(filePath: string, sshHost?: string | null): Promise<Buffer> {
  if (sshHost) {
    const script = "import base64,os,stat,sys\nf=os.fdopen(os.open(sys.argv[1],os.O_RDONLY|os.O_NONBLOCK),'rb')\ns=os.fstat(f.fileno())\nassert stat.S_ISREG(s.st_mode) and s.st_size<=int(sys.argv[2]),'Attachment is not a bounded regular file'\nb=f.read(int(sys.argv[2])+1)\nassert len(b)<=int(sys.argv[2]),'Attachment is too large'\nprint(base64.b64encode(b).decode('ascii'))";
    const encoded = await execGit(`python3 -c ${quoteShellArgument(script)} ${quoteShellArgument(filePath)} ${MAX_ATTACHMENT_BYTES}`, sshHost, { timeoutMs: 15_000, maxBufferBytes: Math.ceil(MAX_ATTACHMENT_BYTES / 3) * 4 + 4096 });
    return Buffer.from(encoded.trim(), "base64");
  }
  const file = await open(filePath, constants.O_RDONLY | constants.O_NONBLOCK);
  try {
    const stats = await file.stat();
    if (!stats.isFile() || stats.size > MAX_ATTACHMENT_BYTES) throw new Error("첨부 파일이 없거나 10MiB를 초과합니다.");
    const chunks: Buffer[] = [];
    let total = 0;
    while (total <= MAX_ATTACHMENT_BYTES) {
      const chunk = Buffer.alloc(Math.min(64 * 1024, MAX_ATTACHMENT_BYTES + 1 - total));
      const { bytesRead } = await file.read(chunk, 0, chunk.length, null);
      if (!bytesRead) break;
      total += bytesRead;
      if (total > MAX_ATTACHMENT_BYTES) throw new Error("첨부 파일이 10MiB를 초과합니다.");
      chunks.push(chunk.subarray(0, bytesRead));
    }
    return Buffer.concat(chunks, total);
  } finally {
    await file.close();
  }
}

export async function readAiSessionAttachment(
  context: AiSessionReaderContext,
  provider: AiSessionProvider,
  sessionId: string,
  attachmentId: string,
  sourceRef?: string | null,
): Promise<AiSessionAttachmentContent | null> {
  if (!/^[a-f0-9]{64}$/.test(attachmentId) || (provider !== "claude" && provider !== "codex")) return null;
  const detail = await getAiSessionDetail(context, provider, sessionId, sourceRef, null, Number.MAX_SAFE_INTEGER);
  if (!detail?.matchedPath || !determineMatchScope(detail.matchedPath, context)) return null;
  const attachment = detail?.messages.flatMap(message => message.attachments ?? []).find(candidate => candidate.id === attachmentId);
  const recordedSource = detail?.sourceRef;
  if (!attachment || !recordedSource) return null;
  const events = await getCachedOrParse(recordedSource, () => readJsonLines(recordedSource, context.sshHost), context.sshHost);
  for (const raw of events) {
    const event = raw as { message?: { content?: unknown }; payload?: { content?: unknown } };
    const recorded = findMessageAttachment(event.message?.content ?? event.payload?.content, recordedSource, attachmentId);
    if (!recorded) continue;
    let decoded: { buffer: Buffer; mimeType: string };
    if (recorded.source.type === "data") {
      decoded = decodeAttachmentDataUrl(recorded.source.dataUrl);
    } else if (recorded.source.type === "path") {
      let buffer: Buffer;
      try {
        buffer = await readRecordedFile(recorded.source.path, context.sshHost);
      } catch (error) {
        if ((error as NodeJS.ErrnoException).code === "ENOENT") return null;
        throw error;
      }
      const mimeType = detectImageMimeType(buffer) ?? attachment.mimeType ?? "application/octet-stream";
      decoded = decodeAttachmentDataUrl(`data:${mimeType};base64,${buffer.toString("base64")}`);
    } else {
      throw new Error("이 기록의 외부 이미지 주소는 불러올 수 없습니다.");
    }
    if (attachment.kind === "image" && !detectImageMimeType(decoded.buffer)) throw new Error("지원하지 않거나 손상된 이미지입니다.");
    return { id: attachment.id, name: attachment.name, mimeType: decoded.mimeType, dataUrl: `data:${decoded.mimeType};base64,${decoded.buffer.toString("base64")}` };
  }
  return null;
}
