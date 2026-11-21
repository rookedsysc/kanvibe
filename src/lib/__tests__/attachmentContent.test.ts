/** @vitest-environment node */
import { describe, expect, it } from "vitest";
import { decodeAttachmentDataUrl, detectImageMimeType, MAX_ATTACHMENT_BYTES } from "../attachmentContent";

const png = Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aOK0AAAAASUVORK5CYII=", "base64");
describe("attachment content boundary", () => {
  it("decodes actual images and ordinary file bytes with their MIME", () => {
    expect(decodeAttachmentDataUrl(`data:image/png;base64,${png.toString("base64")}`, "image/png")).toEqual({ buffer: png, mimeType: "image/png" });
    expect(decodeAttachmentDataUrl("data:text/plain;base64,aGVsbG8=")).toEqual({ buffer: Buffer.from("hello"), mimeType: "text/plain" });
    expect(detectImageMimeType(png)).toBe("image/png");
    expect(detectImageMimeType(Buffer.from("not an image"))).toBeNull();
  });
  it.each(["data:image/png;base64,aGVsbG8=", "data:text/plain;base64,@@@@", "data:text/plain;base64,aGVsbG8", "data:text/plain;base64,", "https://example.com/photo.png"])("rejects malformed or falsely declared bytes: %s", (value) => {
    expect(() => decodeAttachmentDataUrl(value)).toThrow();
  });
  it("rejects MIME disagreement and decoded files over the announced limit", () => {
    expect(() => decodeAttachmentDataUrl("data:text/plain;base64,aGVsbG8=", "image/png")).toThrow();
    expect(() => decodeAttachmentDataUrl(`data:application/octet-stream;base64,${Buffer.alloc(MAX_ATTACHMENT_BYTES + 1).toString("base64")}`)).toThrow();
  });
});
