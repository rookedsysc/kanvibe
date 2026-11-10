// 화면에 열린 기기와 별개로 데스크탑 쪽에서 세션 종류를 바꾼다.
//
// 데스크탑 화면의 선택 박스는 Maestro가 누를 수 없어, 같은 변경 경로(kanbanService.changeTaskSessionType)를
// 타는 모바일 API를 두 번째 기기로 부른다. 화면의 기기는 이 요청에 관여하지 않으므로, 이후 화면이 새 세션
// 종류를 보이면 그것은 데스크탑이 보낸 session-changed 종료(4002)를 받고 스스로 다시 붙었다는 뜻이다.
const SESSION_TASK_TITLE = "Maestro Fixture Session";

function pairOtherDevice() {
  const codeResponse = http.post(`${DESKTOP_API_FROM_HOST}/api/mobile/dev/pairing-code`, { body: "" });
  if (codeResponse.status !== 200) {
    throw new Error(`개발용 페어링 코드를 받지 못했습니다: HTTP ${codeResponse.status}`);
  }
  const pairResponse = http.post(`${DESKTOP_API_FROM_HOST}/api/mobile/pair`, {
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ code: json(codeResponse.body).code, deviceName: "Maestro 데스크탑 대역" }),
  });
  if (pairResponse.status !== 200) {
    throw new Error(`두 번째 기기 페어링 실패: HTTP ${pairResponse.status} ${pairResponse.body}`);
  }
  return json(pairResponse.body).token;
}

function findSessionTaskId(authorization) {
  const boardResponse = http.get(`${DESKTOP_API_FROM_HOST}/api/mobile/board`, { headers: { Authorization: authorization } });
  if (boardResponse.status !== 200) {
    throw new Error(`보드 조회 실패: HTTP ${boardResponse.status}`);
  }
  const tasksByStatus = json(boardResponse.body).board.tasks;
  for (const status of Object.keys(tasksByStatus)) {
    for (const task of tasksByStatus[status]) {
      if (task && task.title === SESSION_TASK_TITLE) {
        return task.id;
      }
    }
  }
  throw new Error(`보드에서 '${SESSION_TASK_TITLE}' 태스크를 찾지 못했습니다`);
}

const authorization = `Bearer ${pairOtherDevice()}`;
const taskId = findSessionTaskId(authorization);

const changeResponse = http.request(`${DESKTOP_API_FROM_HOST}/api/mobile/tasks/${taskId}/session-type`, {
  method: "PATCH",
  headers: { "Content-Type": "application/json", Authorization: authorization },
  body: JSON.stringify({ sessionType: SESSION_TYPE, shouldCloseExistingSession: true }),
});
if (changeResponse.status !== 200) {
  throw new Error(`데스크탑 세션 종류 변경 실패: HTTP ${changeResponse.status} ${changeResponse.body}`);
}

const surfacesResponse = http.get(`${DESKTOP_API_FROM_HOST}/api/mobile/tasks/${taskId}/surfaces`, {
  headers: { Authorization: authorization },
});
const surfaces = json(surfacesResponse.body).surfaces;
if (surfaces.sessionType !== SESSION_TYPE) {
  throw new Error(`데스크탑이 바뀐 세션 종류를 돌려주지 않았습니다: ${surfacesResponse.body}`);
}
output.changedSessionType = surfaces.sessionType;
