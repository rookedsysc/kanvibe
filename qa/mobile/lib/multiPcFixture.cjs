/* eslint-disable @typescript-eslint/no-require-imports */
const http = require("node:http");

// 모바일의 저장/전환 여정만 검증하는 두 개의 격리된 PC API.
// 실제 KanVibe API 계약은 기존 prepareQaDesktop.cjs 여정이 검증한다.
function startPc(port, title) {
  const token = `qa-token-${port}`;
  let paired = false;

  const server = http.createServer(async (request, response) => {
    const json = (status, body) => {
      response.writeHead(status, { "Content-Type": "application/json" });
      response.end(JSON.stringify(body));
    };

    if (request.url === "/health") return json(200, { ready: true });

    if (request.url === "/api/mobile/pair" && request.method === "POST") {
      let body = "";
      for await (const chunk of request) body += chunk;
      if (JSON.parse(body).code !== "123456") return json(401, { error: "wrong code" });
      paired = true;
      return json(200, { token });
    }

    if (!paired || request.headers.authorization !== `Bearer ${token}`) {
      return json(401, { error: "unauthorized" });
    }

    if (request.url === "/api/mobile/board" && request.method === "GET") {
      return json(200, {
        board: {
          projects: [],
          tasks: {
            todo: [{ id: `qa-${port}`, title, status: "todo" }],
            progress: [], pending: [], review: [], done: [],
          },
        },
      });
    }

    if (request.url === "/api/mobile/device" && request.method === "DELETE") {
      paired = false;
      return json(200, { success: true });
    }

    return json(404, { error: "not found" });
  });

  server.listen(port, "127.0.0.1", () => {
    process.stdout.write(`QA PC ${port} ready\n`);
  });
  return server;
}

const servers = [startPc(19836, "Studio task"), startPc(19837, "Home task")];
function close() {
  for (const server of servers) server.close();
}
process.on("SIGINT", close);
process.on("SIGTERM", close);
