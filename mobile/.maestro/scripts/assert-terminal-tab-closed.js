// 닫은 탭의 pane이 데스크탑 tmux에서 실제로 사라졌고 처음 pane은 남아 있어야 한다.
// 앱 화면의 탭 칩이 사라진 것만으로는 소켓 명령이 데스크탑에 닿았다는 증거가 되지 않는다.
const response = http.get(`${PROBE_API_FROM_HOST}/terminal-tabs?marker=`);
if (response.status !== 200) {
  throw new Error(`terminal tab probe 실패: HTTP ${response.status}`);
}
const state = json(response.body);
if (state.windowCount !== 1 || !state.initialPaneRetained || state.newPaneId) {
  throw new Error(`탭이 닫히지 않았거나 처음 pane이 사라짐: ${response.body}`);
}
