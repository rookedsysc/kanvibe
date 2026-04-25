const http = require("node:http");
const path = require("node:path");

function getHookServiceModulePath() {
  if (process.env.KANVIBE_RENDERER_URL) {
    return path.join(process.cwd(), "src", "desktop", "main", "services", "hookService.ts");
  }

  return path.join(process.cwd(), "build", "main", "src", "desktop", "main", "services", "hookService.js");
}

function readJsonBody(request) {
  return new Promise((resolve, reject) => {
    let body = "";
    request.on("data", (chunk) => {
      body += chunk;
    });
    request.on("end", () => {
      try {
        resolve(body ? JSON.parse(body) : {});
      } catch (error) {
        reject(error);
      }
    });
    request.on("error", reject);
  });
}

function writeJson(response, statusCode, payload) {
  response.writeHead(statusCode, { "Content-Type": "application/json" });
  response.end(JSON.stringify(payload));
}

function createHookServer({ host, port }) {
  const hookService = require(getHookServiceModulePath());

  const server = http.createServer(async (request, response) => {
    if (request.method === "GET" && request.url === "/api/hooks/health") {
      writeJson(response, 200, { success: true });
      return;
    }

    if (request.method === "POST" && request.url === "/api/hooks/start") {
      try {
        const result = await hookService.startHookTask(await readJsonBody(request));
        writeJson(response, result.status || 200, result);
      } catch (error) {
        writeJson(response, 500, { success: false, error: error instanceof Error ? error.message : "서버 오류" });
      }
      return;
    }

    if (request.method === "POST" && request.url === "/api/hooks/status") {
      try {
        const result = await hookService.updateHookTaskStatus(await readJsonBody(request));
        writeJson(response, result.status || 200, result);
      } catch (error) {
        writeJson(response, 500, { success: false, error: error instanceof Error ? error.message : "서버 오류" });
      }
      return;
    }

    writeJson(response, 404, { success: false, error: "Not found" });
  });

  server.listen(port, host, () => {
    const logUrl = host === "0.0.0.0"
      ? `http://localhost:${port} (bound to ${host}:${port})`
      : `http://${host}:${port}`;
    console.log(`[kanvibe] Hook server listening on ${logUrl}`);
  });

  server.on("error", (error) => {
    console.error("[kanvibe] Hook server failed:", error);
  });

  return server;
}

module.exports = { createHookServer };
