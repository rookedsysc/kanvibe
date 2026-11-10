const { readJsonBody, writeJson } = require("./httpBody.js");

/** task CLI가 부르는 경로와 그 경로를 처리하는 서비스 함수 이름 */
const TASK_CLI_ROUTES = {
  "/api/cli/tasks": { serviceFunctionName: "createTaskFromCli", successStatus: 201 },
  "/api/cli/tasks/delete": { serviceFunctionName: "deleteTaskFromCli", successStatus: 200 },
  "/api/cli/tasks/register": { serviceFunctionName: "registerTaskFromPeer", successStatus: 200 },
};

/**
 * task CLI의 생성·삭제 요청을 처리한다. 처리한 경로면 true를 돌려준다.
 *
 * 인증은 서비스가 targets.json에 기록한 task 단위 토큰으로 한다. 토큰은 worktree 파일을 읽을 수 있어야 얻으므로
 * 인증 뒤의 오류 메시지에 git 출력이 섞여도 요청자가 이미 볼 수 있는 범위를 넘지 않아, CLI가 원인을 보여 줄 수 있게 그대로 돌려준다.
 * 인증 전에 난 내부 오류는 서비스가 원인을 감춘 메시지로 바꿔 보낸다.
 */
/** @param sourceSshHosts 터널로 들어온 요청이면 그 터널이 맡은 호스트들. 평문 포트의 요청에는 없다 */
async function handleTaskCliRequest(request, response, { taskCli, sourceSshHosts }) {
  const route = request.method === "POST" ? TASK_CLI_ROUTES[request.url] : undefined;
  if (!route) {
    return false;
  }

  let body;
  try {
    body = await readJsonBody(request);
  } catch {
    writeJson(response, 400, { success: false, error: "JSON 본문을 읽을 수 없습니다" });
    return true;
  }

  if (!body || typeof body !== "object" || Array.isArray(body)) {
    writeJson(response, 400, { success: false, error: "JSON 객체 본문이 필요합니다" });
    return true;
  }

  try {
    const data = await taskCli[route.serviceFunctionName](request.headers.authorization, body, sourceSshHosts);
    writeJson(response, route.successStatus, { success: true, data });
  } catch (error) {
    if (error && error.name === "TaskCliRequestError") {
      writeJson(response, error.statusCode, { success: false, error: error.message });
      return true;
    }

    console.error(`[kanvibe] task CLI ${request.url} failed:`, error);
    writeJson(response, 500, { success: false, error: error instanceof Error ? error.message : "서버 오류" });
  }

  return true;
}

module.exports = { handleTaskCliRequest };
