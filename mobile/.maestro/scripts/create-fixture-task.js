// 보드에서 찾을 태스크를 이번 실행마다 새로 만든다. 세션 없이 만들어 상세 화면의 안내가 매번 같게 한다.
const fixtureTaskTitle = `Maestro E2E ${Date.now()}`;
const response = http.post(`${DESKTOP_API_FROM_HOST}/api/hooks/start`, {
  headers: { "Content-Type": "application/json" },
  body: JSON.stringify({ title: fixtureTaskTitle }),
});
if (response.status !== 200) {
  throw new Error(`픽스처 태스크를 만들지 못했습니다: HTTP ${response.status}`);
}
output.fixtureTaskTitle = fixtureTaskTitle;
