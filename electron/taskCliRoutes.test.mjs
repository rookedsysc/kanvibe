/**
 * @vitest-environment node
 */
import http from "node:http";
import { createRequire } from "node:module";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const require = createRequire(import.meta.url);
const { handleTaskCliRequest } = require("./taskCliRoutes");

let server;
let port;
let taskCli;

class TaskCliRequestError extends Error {
  constructor(message, statusCode) {
    super(message);
    this.name = "TaskCliRequestError";
    this.statusCode = statusCode;
  }
}

async function startServer() {
  server = http.createServer(async (request, response) => {
    if (await handleTaskCliRequest(request, response, { taskCli })) {
      return;
    }
    response.writeHead(418);
    response.end();
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  port = server.address().port;
}

async function send(method, path, body, headers = {}) {
  const response = await fetch(`http://127.0.0.1:${port}${path}`, {
    method,
    headers: { "Content-Type": "application/json", ...headers },
    body,
  });
  const text = await response.text();
  return { status: response.status, body: text ? JSON.parse(text) : null };
}

describe("taskCliRoutes", () => {
  beforeEach(async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    taskCli = {
      createTaskFromCli: vi.fn(async () => ({ id: "created-task" })),
      deleteTaskFromCli: vi.fn(async () => ({ id: "deleted-task", deleted: true })),
      registerTaskFromPeer: vi.fn(async () => ({ id: "registered-task" })),
    };
    await startServer();
  });

  afterEach(async () => {
    await new Promise((resolve) => server.close(resolve));
    vi.restoreAllMocks();
  });

  it.each([
    ["/api/cli/tasks", "createTaskFromCli", 201, { id: "created-task" }],
    ["/api/cli/tasks/delete", "deleteTaskFromCli", 200, { id: "deleted-task", deleted: true }],
    ["/api/cli/tasks/register", "registerTaskFromPeer", 200, { id: "registered-task" }],
  ])("POST %s 는 Authorization 헤더와 본문을 %s 에 넘기고 결과를 돌려준다", async (path, functionName, status, data) => {
    const result = await send("POST", path, JSON.stringify({ contextTaskId: "task-1" }), { Authorization: "Bearer token-1" });

    expect(result).toEqual({ status, body: { success: true, data } });
    expect(taskCli[functionName]).toHaveBeenCalledWith("Bearer token-1", { contextTaskId: "task-1" }, undefined);
  });

  it("서비스가 거절한 요청은 그 응답 코드와 사유로 답한다", async () => {
    taskCli.deleteTaskFromCli.mockRejectedValue(new TaskCliRequestError("task CLI 인증에 실패했습니다.", 401));

    const result = await send("POST", "/api/cli/tasks/delete", JSON.stringify({ contextTaskId: "task-1" }));

    expect(result).toEqual({ status: 401, body: { success: false, error: "task CLI 인증에 실패했습니다." } });
  });

  it("예상하지 못한 실패는 500과 원인 메시지로 답해 CLI가 git 오류를 보여 줄 수 있게 한다", async () => {
    taskCli.createTaskFromCli.mockRejectedValue(new Error("fatal: invalid reference: missing"));

    const result = await send("POST", "/api/cli/tasks", JSON.stringify({ contextTaskId: "task-1" }));

    expect(result).toEqual({ status: 500, body: { success: false, error: "fatal: invalid reference: missing" } });
  });

  it.each([
    ["{not json", "JSON 본문을 읽을 수 없습니다"],
    ["[1,2]", "JSON 객체 본문이 필요합니다"],
  ])("본문 %s 은 서비스에 넘기지 않고 400으로 거절한다", async (body, error) => {
    const result = await send("POST", "/api/cli/tasks", body);

    expect(result).toEqual({ status: 400, body: { success: false, error } });
    expect(taskCli.createTaskFromCli).not.toHaveBeenCalled();
  });

  it("다른 경로와 메서드는 처리하지 않고 hook 서버 라우팅에 넘긴다", async () => {
    expect((await fetch(`http://127.0.0.1:${port}/api/cli/tasks`)).status).toBe(418);
    expect((await fetch(`http://127.0.0.1:${port}/api/hooks/status`, { method: "POST", body: "{}" })).status).toBe(418);
  });
});
