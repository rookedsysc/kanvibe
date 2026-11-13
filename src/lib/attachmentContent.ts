export const MAX_ATTACHMENT_BYTES = 10 * 1024 * 1024;

export function detectImageMimeType(buffer: Buffer): string | null {
  if (buffer.length >= 8 && buffer.subarray(0, 8).equals(Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]))) return "image/png";
  if (buffer.length >= 3 && buffer[0] === 255 && buffer[1] === 216 && buffer[2] === 255) return "image/jpeg";
  if (buffer.length >= 6 && ["GIF87a", "GIF89a"].includes(buffer.toString("ascii", 0, 6))) return "image/gif";
  if (buffer.length >= 12 && buffer.toString("ascii", 0, 4) === "RIFF" && buffer.toString("ascii", 8, 12) === "WEBP") return "image/webp";
  return null;
}

export function decodeAttachmentDataUrl(dataUrl: string, expectedMimeType?: string): { buffer: Buffer; mimeType: string } {
  if (typeof dataUrl !== "string") throw new Error("첨부 파일 데이터가 올바르지 않습니다.");
  const comma = dataUrl.indexOf(",");
  if (comma < 0 || comma > 200) throw new Error("첨부 파일 형식이 올바르지 않습니다.");
  const header = /^data:([a-z0-9!#$&^_.+-]+\/[a-z0-9!#$&^_.+-]+);base64$/i.exec(dataUrl.slice(0, comma));
  if (!header) throw new Error("Base64 첨부 파일만 지원합니다.");
  const mimeType = header[1].toLowerCase().replace(/^image\/jpg$/, "image/jpeg");
  const expected = expectedMimeType?.toLowerCase().replace(/^image\/jpg$/, "image/jpeg");
  if (expected !== undefined && expected !== mimeType) throw new Error("첨부 파일 MIME 형식이 일치하지 않습니다.");
  const payload = dataUrl.slice(comma + 1);
  if (!payload || payload.length > Math.ceil(MAX_ATTACHMENT_BYTES / 3) * 4 || payload.length % 4 !== 0 || !/^[A-Za-z0-9+/]*={0,2}$/.test(payload)) throw new Error("첨부 파일이 비어 있거나 크기·인코딩이 올바르지 않습니다.");
  const buffer = Buffer.from(payload, "base64");
  if (!buffer.length || buffer.length > MAX_ATTACHMENT_BYTES || buffer.toString("base64") !== payload) throw new Error("첨부 파일이 비어 있거나 10MiB를 초과합니다.");
  const detected = detectImageMimeType(buffer);
  if (mimeType.startsWith("image/") && detected !== mimeType) throw new Error("지원하지 않거나 손상된 이미지입니다. PNG, JPEG, GIF, WebP를 사용해 주세요.");
  return { buffer, mimeType: mimeType === "application/octet-stream" && detected ? detected : mimeType };
}
