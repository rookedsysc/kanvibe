export type MobileDiagramInput = { definition: string; theme: "light" | "dark"; width: number };
export type MobileDiagramImage = { imageBase64: string; width: number; height: number; mimeType: "image/png" | "image/jpeg" };
export const MOBILE_DIAGRAM_LIMITS = { definitionBytes: 65536, widthMin: 320, widthMax: 1200, heightMax: 2400, imageBytes: 4 * 1024 * 1024, pixels: 2880000, pending: 4, cacheEntries: 16, cacheBytes: 16 * 1024 * 1024, timeoutMs: 15000 } as const;

export class MobileDiagramError extends Error {
  constructor(public readonly statusCode: 400 | 422 | 429 | 500 | 503, message: string) {
    super(message);
    this.name = "MobileDiagramError";
  }
}

export function parseMobileDiagramInput(value: unknown): MobileDiagramInput {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new MobileDiagramError(400, "잘못된 다이어그램 요청입니다");
  const { definition, theme, width } = value as Record<string, unknown>;
  if (typeof definition !== "string" || !definition.trim() || Buffer.byteLength(definition, "utf8") > MOBILE_DIAGRAM_LIMITS.definitionBytes || (theme !== "light" && theme !== "dark") || typeof width !== "number" || !Number.isInteger(width) || width < 320 || width > 1200) throw new MobileDiagramError(400, "잘못된 다이어그램 요청입니다");
  // Request data cannot override the renderer's security, resource limits or theme.
  if (/%%\s*\{|^\s*---/m.test(definition)) throw new MobileDiagramError(400, "다이어그램 설정 지시문은 지원하지 않습니다");
  return { definition, theme: theme as "light" | "dark", width };
}

function jpegDimensions(image: Buffer): { width: number; height: number } {
  const invalid = () => new MobileDiagramError(500, "다이어그램 이미지가 올바르지 않습니다");
  if (image.length < 4 || image[0] !== 0xff || image[1] !== 0xd8 || image[image.length - 2] !== 0xff || image[image.length - 1] !== 0xd9) throw invalid();
  let offset = 2;
  let dimensions: { width: number; height: number } | undefined;
  while (offset < image.length - 2) {
    if (image[offset++] !== 0xff) throw invalid();
    while (offset < image.length && image[offset] === 0xff) offset++;
    const marker = image[offset++];
    if (marker === undefined || marker === 0 || marker === 0xd8 || marker === 0xd9 || marker === 0x01 || (marker >= 0xd0 && marker <= 0xd7) || offset + 2 > image.length - 2) throw invalid();
    const length = image.readUInt16BE(offset);
    if (length < 2 || offset + length > image.length - 2) throw invalid();
    const frameMarker = marker >= 0xc0 && marker <= 0xcf && ![0xc4, 0xc8, 0xcc].includes(marker);
    if (frameMarker && ![0xc0, 0xc1, 0xc2].includes(marker)) throw invalid();
    if ([0xc0, 0xc1, 0xc2].includes(marker)) {
      if (dimensions || length < 11 || image[offset + 2] !== 8) throw invalid();
      const components = image[offset + 7];
      if (![1, 3].includes(components) || length !== 8 + components * 3) throw invalid();
      dimensions = { height: image.readUInt16BE(offset + 3), width: image.readUInt16BE(offset + 5) };
    }
    if (marker === 0xda) {
      if (!dimensions || length < 6 || image[offset + 2] < 1 || image[offset + 2] > 3 || length !== 6 + image[offset + 2] * 2 || offset + length >= image.length - 2) throw invalid();
      return dimensions;
    }
    offset += length;
  }
  throw invalid();
}

export function validateMobileDiagramImage(image: Buffer, expectedWidth: number): MobileDiagramImage {
  if (!image.length || image.length > MOBILE_DIAGRAM_LIMITS.imageBytes) throw new MobileDiagramError(500, "다이어그램 이미지를 만들지 못했습니다");
  const isPng = image.length >= 33 && image.subarray(0, 8).equals(Buffer.from([137, 80, 78, 71, 13, 10, 26, 10])) && image.toString("ascii", 12, 16) === "IHDR";
  const { width, height } = isPng ? { width: image.readUInt32BE(16), height: image.readUInt32BE(20) } : jpegDimensions(image);
  if (width !== expectedWidth || height < 1 || height > 2400 || width * height > MOBILE_DIAGRAM_LIMITS.pixels) throw new MobileDiagramError(500, "다이어그램 이미지 크기가 올바르지 않습니다");
  return { imageBase64: image.toString("base64"), width, height, mimeType: isPng ? "image/png" : "image/jpeg" };
}
