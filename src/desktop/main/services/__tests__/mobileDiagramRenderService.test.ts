import { beforeEach, describe, expect, it, vi } from "vitest";
import { MOBILE_DIAGRAM_LIMITS, MobileDiagramError, parseMobileDiagramInput, validateMobileDiagramImage } from "../../mobileDiagramContract";

const native = vi.hoisted(() => ({ render: vi.fn() }));
vi.mock("../../mobileDiagramWorker", () => ({ renderDiagramInDesktop: native.render }));
const input = { definition: "flowchart LR\nA[Start]-->B[Done]", theme: "light", width: 640 };
function png(width = 640, height = 120) {
  const bytes = Buffer.alloc(33);
  Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]).copy(bytes);
  bytes.write("IHDR", 12);
  bytes.writeUInt32BE(width, 16);
  bytes.writeUInt32BE(height, 20);
  return bytes;
}
const validTinyJpeg = Buffer.from("/9j/4AAQSkZJRgABAQAAAQABAAD/2wBDAAIBAQEBAQIBAQECAgICAgQDAgICAgUEBAMEBgUGBgYFBgYGBwkIBgcJBwYGCAsICQoKCgoKBggLDAsKDAkKCgr/2wBDAQICAgICAgUDAwUKBwYHCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgr/wAARCAACAAIDAREAAhEBAxEB/8QAHwAAAQUBAQEBAQEAAAAAAAAAAAECAwQFBgcICQoL/8QAtRAAAgEDAwIEAwUFBAQAAAF9AQIDAAQRBRIhMUEGE1FhByJxFDKBkaEII0KxwRVS0fAkM2JyggkKFhcYGRolJicoKSo0NTY3ODk6Q0RFRkdISUpTVFVWV1hZWmNkZWZnaGlqc3R1dnd4eXqDhIWGh4iJipKTlJWWl5iZmqKjpKWmp6ipqrKztLW2t7i5usLDxMXGx8jJytLT1NXW19jZ2uHi4+Tl5ufo6erx8vP09fb3+Pn6/8QAHwEAAwEBAQEBAQEBAQAAAAAAAAECAwQFBgcICQoL/8QAtREAAgECBAQDBAcFBAQAAQJ3AAECAxEEBSExBhJBUQdhcRMiMoEIFEKRobHBCSMzUvAVYnLRChYkNOEl8RcYGRomJygpKjU2Nzg5OkNERUZHSElKU1RVVldYWVpjZGVmZ2hpanN0dXZ3eHl6goOEhYaHiImKkpOUlZaXmJmaoqOkpaanqKmqsrO0tba3uLm6wsPExcbHyMnK0tPU1dbX2Nna4uPk5ebn6Onq8vP09fb3+Pn6/9oADAMBAAIRAxEAPwD9YP8AgnB8CPgf4l/4J4/AXxF4i+DXhTUNQ1D4L+Frm/v73w7bSzXMz6Tas8kjshZ3ZiWLEkkkk16Hij4e8A4XxMzuhQynDQhDGYlRiqFJJJVppJJQskloktEj4/EcF8HZ/XnmeZ5bh6+Jrt1KtWpRpzqVKk3zTqVJyi5TnOTcpzk3KUm222z/2Q==", "base64");
function jpeg(width = 640, height = 120) {
  const image = Buffer.from(validTinyJpeg);
  const marker = image.indexOf(Buffer.from([0xff, 0xc0]));
  image.writeUInt16BE(height, marker + 5);
  image.writeUInt16BE(width, marker + 7);
  return image;
}
beforeEach(() => { vi.resetModules(); native.render.mockReset(); native.render.mockResolvedValue(jpeg()); });

describe("diagram input and native image boundary", () => {
  it("rejects four-component scans even when their segment length and entropy bounds are consistent", () => {
    const image = jpeg();
    const scan = image.indexOf(Buffer.from([0xff, 0xda]));
    const oldLength = image.readUInt16BE(scan + 2);
    const fourComponents = Buffer.from([0,14,4,1,0,2,0,3,0,4,0,0,63,0]);
    const invalid = Buffer.concat([image.subarray(0, scan + 2), fourComponents, image.subarray(scan + 2 + oldLength)]);
    expect(() => validateMobileDiagramImage(invalid, 640)).toThrow();
  });

  it("recognizes a real JPEG container and preserves explicit MIME plus SOF dimensions", () => {
    expect(validateMobileDiagramImage(validTinyJpeg, 2)).toMatchObject({ width: 2, height: 2, mimeType: "image/jpeg" });
    expect(validateMobileDiagramImage(jpeg(1200, 2400), 1200)).toMatchObject({ width: 1200, height: 2400, mimeType: "image/jpeg" });
    expect(validateMobileDiagramImage(png(), 640).mimeType).toBe("image/png");
  });
  it("rejects JPEG size mismatches, missing EOI, malformed lengths and unsupported frame formats", () => {
    expect(() => validateMobileDiagramImage(jpeg(639), 640)).toThrow();
    expect(() => validateMobileDiagramImage(jpeg(640, 2401), 640)).toThrow();
    expect(() => validateMobileDiagramImage(jpeg().subarray(0, -2), 640)).toThrow();
    const badLength = jpeg(); badLength.writeUInt16BE(1, 4);
    expect(() => validateMobileDiagramImage(badLength, 640)).toThrow();
    const overflowLength = jpeg(); overflowLength.writeUInt16BE(65535, 4);
    expect(() => validateMobileDiagramImage(overflowLength, 640)).toThrow();
    const lossless = jpeg(); lossless[lossless.indexOf(Buffer.from([0xff, 0xc0])) + 1] = 0xc3;
    expect(() => validateMobileDiagramImage(lossless, 640)).toThrow();
    const precision = jpeg(); precision[precision.indexOf(Buffer.from([0xff, 0xc0])) + 4] = 12;
    expect(() => validateMobileDiagramImage(precision, 640)).toThrow();
    expect(() => validateMobileDiagramImage(Buffer.concat([Buffer.from([0xff,0xd8]), Buffer.alloc(MOBILE_DIAGRAM_LIMITS.imageBytes)]), 640)).toThrow();
  });
  it("does not accept a dimensions-only fake JPEG without a scan", () => {
    const image = jpeg();
    const scan = image.indexOf(Buffer.from([0xff, 0xda]));
    expect(() => validateMobileDiagramImage(Buffer.concat([image.subarray(0, scan), Buffer.from([0xff, 0xd9])]), 640)).toThrow();
  });

  it.each([null, [], {}, { ...input, width: 319 }, { ...input, width: 1201 }, { ...input, width: 640.5 }, { ...input, theme: "neutral" }, { ...input, theme: ["light"] }, { ...input, theme: { value: "dark" } }, { ...input, definition: " " }, { ...input, definition: "가".repeat(21846) }, { ...input, definition: "%%{init: {securityLevel: 'loose'}}%%\nflowchart LR\nA-->B" }, { ...input, definition: "---\nconfig: {}\n---\nflowchart LR\nA-->B" }])("rejects unsafe/invalid input %j", (value) => {
    expect(() => parseMobileDiagramInput(value)).toThrowError(MobileDiagramError);
  });
  it("accepts exact UTF-8/source and width boundaries", () => {
    expect(parseMobileDiagramInput({ ...input, width: 320, definition: "a".repeat(65536) }).definition.length).toBe(65536);
    expect(parseMobileDiagramInput({ ...input, theme: "dark", width: 1200 }).theme).toBe("dark");
  });
  it("reads actual PNG dimensions and rejects mismatches, invalid magic, height and bytes", () => {
    expect(validateMobileDiagramImage(png(), 640)).toMatchObject({ width: 640, height: 120 });
    expect(() => validateMobileDiagramImage(png(639), 640)).toThrow();
    expect(() => validateMobileDiagramImage(png(640, 2401), 640)).toThrow();
    expect(() => validateMobileDiagramImage(Buffer.alloc(33), 640)).toThrow();
    expect(() => validateMobileDiagramImage(Buffer.concat([png(), Buffer.alloc(MOBILE_DIAGRAM_LIMITS.imageBytes)]), 640)).toThrow();
  });
});

describe("desktop renderer capacity and result lifecycle", () => {
  it("evicts by encoded image bytes before the entry count is reached", async () => {
    const { renderMobileDiagram } = await import("../mobileDiagramRenderService");
    const large = Buffer.concat([png(), Buffer.alloc(3 * 1024 * 1024 - 33)]);
    native.render.mockResolvedValue(large);
    await renderMobileDiagram(input);
    for (let index = 0; index < 4; index++) await renderMobileDiagram({ ...input, definition: `flowchart LR\nMemory${index}-->Bound` });
    await renderMobileDiagram(input);
    expect(native.render).toHaveBeenCalledTimes(6);
  });

  it("deduplicates concurrent requests and serves immutable cached copies", async () => {
    const { renderMobileDiagram } = await import("../mobileDiagramRenderService");
    let complete!: (value: Buffer) => void;
    native.render.mockImplementationOnce(() => new Promise((resolve) => { complete = resolve; }));
    const first = renderMobileDiagram(input);
    const second = renderMobileDiagram(input);
    await vi.waitFor(() => expect(native.render).toHaveBeenCalledTimes(1));
    complete(jpeg());
    const [one, two] = await Promise.all([first, second]);
    expect(one).toEqual(two);
    one.height = 999;
    expect((await renderMobileDiagram(input)).height).toBe(120);
    expect(native.render).toHaveBeenCalledTimes(1);
  });
  it("runs one worker, admits three waiting jobs and rejects a fifth distinct job", async () => {
    const { renderMobileDiagram } = await import("../mobileDiagramRenderService");
    const completions: Array<(value: Buffer) => void> = [];
    native.render.mockImplementation(() => new Promise((resolve) => completions.push(resolve)));
    const jobs = [0, 1, 2, 3].map((index) => renderMobileDiagram({ ...input, definition: `flowchart LR\nA-->B${index}` }));
    expect(() => renderMobileDiagram({ ...input, definition: "flowchart LR\nX-->Y" })).toThrowError(/요청이 많습니다/);
    await vi.waitFor(() => expect(completions).toHaveLength(1));
    completions[0](jpeg());
    await vi.waitFor(() => expect(completions).toHaveLength(2));
    completions[1](jpeg());
    await vi.waitFor(() => expect(completions).toHaveLength(3));
    completions[2](jpeg());
    await vi.waitFor(() => expect(completions).toHaveLength(4));
    completions[3](jpeg());
    expect(await Promise.all(jobs)).toHaveLength(4);
  });
  it("does not cache failures and allows the next queued request to run", async () => {
    const { renderMobileDiagram } = await import("../mobileDiagramRenderService");
    native.render.mockRejectedValueOnce(new MobileDiagramError(503, "unavailable"));
    await expect(renderMobileDiagram(input)).rejects.toMatchObject({ statusCode: 503 });
    expect((await renderMobileDiagram(input)).width).toBe(640);
    expect(native.render).toHaveBeenCalledTimes(2);
  });
  it("keeps theme/width distinct and evicts the oldest of more than sixteen images", async () => {
    const { renderMobileDiagram } = await import("../mobileDiagramRenderService");
    await renderMobileDiagram(input);
    await renderMobileDiagram({ ...input, theme: "dark" });
    for (let index = 0; index < 15; index++) await renderMobileDiagram({ ...input, definition: `flowchart LR\nN${index}-->Z` });
    await renderMobileDiagram(input);
    expect(native.render).toHaveBeenCalledTimes(18);
  });
});
