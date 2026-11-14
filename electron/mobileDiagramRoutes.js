const { readJsonBody, writeJson } = require("./httpBody.js");

async function handleMobileDiagramRequest(request, response, { bridge, requestUrl }) {
  if (request.method !== "POST" || requestUrl.pathname !== "/api/mobile/diagrams/render") return false;
  try {
    let body;
    try {
      // JSON escapes may expand the 64 KiB decoded definition sixfold.
      body = await readJsonBody(request, 400 * 1024);
    } catch {
      writeJson(response, 400, { success: false, error: "잘못된 다이어그램 요청입니다" });
      return true;
    }
    const result = await bridge.renderMobileDiagram(body);
    writeJson(response, 200, { success: true, ...result });
  } catch (error) {
    const allowed = error?.name === "MobileDiagramError" && [400, 422, 429, 500, 503].includes(error.statusCode);
    writeJson(response, allowed ? error.statusCode : 500, { success: false, error: allowed ? error.message : "다이어그램 이미지를 만들지 못했습니다" });
  }
  return true;
}

module.exports = { handleMobileDiagramRequest };
