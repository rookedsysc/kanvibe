import { EventEmitter } from "node:events";
import { createRequire } from "node:module";
import { describe, expect, it, vi } from "vitest";
const { handleMobileRequest } = createRequire(import.meta.url)("./mobileRoutes.js");
async function run(bridge, body, authorization = "Bearer valid") {
  const request = new EventEmitter();
  Object.assign(request, { method: "POST", url: "/api/mobile/diagrams/render", headers: { authorization }, socket: { remoteAddress: "127.0.0.1" }, destroy: vi.fn() });
  let status; let payload;
  const response = { writeHead: (code) => { status = code; }, end: (json) => { payload = JSON.parse(json); } };
  setImmediate(() => { request.emit("data", typeof body === "string" ? body : JSON.stringify(body)); request.emit("end"); });
  await handleMobileRequest(request, response, { bridge, host: "127.0.0.1", port: 4885 });
  return { status, payload };
}
const input = { definition: "flowchart LR\nA-->B", theme: "dark", width: 640 };
function fixture() {
  return { authorizeMobileRequest: vi.fn(async (header) => header === "Bearer valid" ? "device" : null), renderMobileDiagram: vi.fn(async () => ({ imageBase64: "jpeg", width: 640, height: 120, mimeType: "image/jpeg" })) };
}
describe("paired desktop image route", () => {
  it("authorizes before touching the diagram provider", async () => {
    const bridge = fixture();
    expect((await run(bridge, input, "Bearer bad")).status).toBe(401);
    expect(bridge.renderMobileDiagram).not.toHaveBeenCalled();
  });
  it("returns only the defined image envelope", async () => {
    const bridge = fixture();
    expect(await run(bridge, input)).toEqual({ status: 200, payload: { success: true, imageBase64: "jpeg", width: 640, height: 120, mimeType: "image/jpeg" } });
    expect(bridge.renderMobileDiagram).toHaveBeenCalledWith(input);
  });
  it("rejects malformed JSON before rendering", async () => {
    const bridge = fixture();
    expect((await run(bridge, "{")).status).toBe(400);
    expect(bridge.renderMobileDiagram).not.toHaveBeenCalled();
  });
  it.each([400, 422, 429, 500, 503])("preserves safe provider error status %d", async (statusCode) => {
    const bridge = fixture();
    bridge.renderMobileDiagram.mockRejectedValueOnce(Object.assign(new Error("safe message"), { name: "MobileDiagramError", statusCode }));
    expect(await run(bridge, input)).toEqual({ status: statusCode, payload: { success: false, error: "safe message" } });
  });
  it("hides arbitrary renderer paths and source from unexpected errors", async () => {
    const bridge = fixture();
    bridge.renderMobileDiagram.mockRejectedValueOnce(new Error("/private/path graph secret"));
    expect(await run(bridge, input)).toEqual({ status: 500, payload: { success: false, error: "다이어그램 이미지를 만들지 못했습니다" } });
  });
});
