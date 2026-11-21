import { createHash } from "node:crypto";
import path from "node:path";
import type { AiMessageAttachment } from "./types";

type AttachmentSource = { type: "data"; dataUrl: string } | { type: "path"; path: string } | { type: "unavailable"; reference: string };
interface RecordedAttachment { kind: "image" | "file"; name: string; mimeType: string | null; source: AttachmentSource }

function recordedAttachments(content: unknown): RecordedAttachment[] {
  if (Array.isArray(content)) return content.flatMap(recordedAttachments);
  if (!content || typeof content !== "object") return [];
  const part = content as Record<string, unknown>;
  const kind = ["image", "input_image", "local_image", "image_url"].includes(String(part.type)) ? "image"
    : ["document", "file", "input_file"].includes(String(part.type)) ? "file" : null;
  if (!kind) return part.content ? recordedAttachments(part.content) : [];
  const source = part.source && typeof part.source === "object" ? part.source as Record<string, unknown> : null;
  const mediaType = typeof source?.media_type === "string" ? source.media_type : typeof part.mime_type === "string" ? part.mime_type : null;
  const imageUrl = typeof part.image_url === "string" ? part.image_url
    : part.image_url && typeof part.image_url === "object" ? (part.image_url as Record<string, unknown>).url : null;
  const dataUrl = source?.type === "base64" && typeof source.data === "string"
    ? `data:${mediaType ?? "application/octet-stream"};base64,${source.data}`
    : typeof imageUrl === "string" && imageUrl.startsWith("data:") ? imageUrl
    : typeof part.file_data === "string" && part.file_data.startsWith("data:") ? part.file_data : null;
  const filePath = typeof part.path === "string" ? part.path : typeof part.file_path === "string" ? part.file_path : null;
  const extensionTypes: Record<string, string> = { ".png": "image/png", ".jpg": "image/jpeg", ".jpeg": "image/jpeg", ".gif": "image/gif", ".webp": "image/webp", ".pdf": "application/pdf", ".txt": "text/plain" };
  const mimeType = mediaType ?? (dataUrl ? /^data:([^;,]+)/.exec(dataUrl)?.[1] ?? null : filePath ? extensionTypes[path.extname(filePath).toLowerCase()] ?? null : null);
  const rawName = typeof part.name === "string" ? part.name : typeof part.filename === "string" ? part.filename : filePath;
  const name = rawName ? path.basename(rawName.replaceAll("\\", "/")) : kind === "image" ? `image.${mimeType?.split("/")[1] ?? "png"}` : "attachment";
  return [{ kind, name, mimeType, source: dataUrl ? { type: "data", dataUrl } : filePath && path.isAbsolute(filePath) ? { type: "path", path: filePath } : { type: "unavailable", reference: typeof imageUrl === "string" ? imageUrl : "" } }];
}

function attachmentMetadata(record: RecordedAttachment, sourceRef: string, index: number): AiMessageAttachment {
  const reference = record.source.type === "data" ? record.source.dataUrl : record.source.type === "path" ? record.source.path : record.source.reference;
  const id = createHash("sha256").update(sourceRef).update("\0").update(String(index)).update("\0").update(reference).digest("hex");
  const payload = record.source.type === "data" ? record.source.dataUrl.split(",", 2)[1] : null;
  return { id, kind: record.kind, name: record.name, mimeType: record.mimeType, sizeBytes: payload ? Math.floor(payload.length * 3 / 4) - (payload.endsWith("==") ? 2 : payload.endsWith("=") ? 1 : 0) : null };
}

export function extractMessageAttachments(content: unknown, sourceRef: string): AiMessageAttachment[] {
  return recordedAttachments(content).map((record, index) => attachmentMetadata(record, sourceRef, index));
}

export function findMessageAttachment(content: unknown, sourceRef: string, attachmentId: string) {
  for (const [index, record] of recordedAttachments(content).entries()) {
    const attachment = attachmentMetadata(record, sourceRef, index);
    if (attachment.id === attachmentId) return { attachment, source: record.source };
  }
  return null;
}
