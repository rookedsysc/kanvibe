import { createHash } from "node:crypto";
import { renderDiagramInDesktop } from "../mobileDiagramWorker";
import { MOBILE_DIAGRAM_LIMITS, MobileDiagramError, parseMobileDiagramInput, validateMobileDiagramImage, type MobileDiagramImage } from "../mobileDiagramContract";

const cache = new Map<string, { image: MobileDiagramImage; bytes: number }>();
const inFlight = new Map<string, Promise<MobileDiagramImage>>();
let cacheBytes = 0;
let queue: Promise<unknown> = Promise.resolve();

export function renderMobileDiagram(value: unknown): Promise<MobileDiagramImage> {
  const input = parseMobileDiagramInput(value);
  const key = createHash("sha256").update(JSON.stringify(input)).digest("hex");
  const cached = cache.get(key);
  if (cached) {
    cache.delete(key);
    cache.set(key, cached);
    return Promise.resolve({ ...cached.image });
  }
  const existing = inFlight.get(key);
  if (existing) return existing.then((image) => ({ ...image }));
  if (inFlight.size >= MOBILE_DIAGRAM_LIMITS.pending) throw new MobileDiagramError(429, "다이어그램 요청이 많습니다. 잠시 후 다시 시도해 주세요");
  const job = queue.catch(() => undefined).then(async () => {
    const encoded = await renderDiagramInDesktop(input);
    const image = validateMobileDiagramImage(encoded, input.width);
    const bytes = Buffer.byteLength(image.imageBase64);
    cache.set(key, { image, bytes });
    cacheBytes += bytes;
    while (cache.size > MOBILE_DIAGRAM_LIMITS.cacheEntries || cacheBytes > MOBILE_DIAGRAM_LIMITS.cacheBytes) {
      const oldest = cache.keys().next().value!;
      cacheBytes -= cache.get(oldest)!.bytes;
      cache.delete(oldest);
    }
    return image;
  });
  inFlight.set(key, job);
  queue = job;
  return job.finally(() => inFlight.delete(key)).then((image) => ({ ...image }));
}
