const response = http.get(`${PROBE_API_FROM_HOST}/terminal-tabs?marker=${MARKER}`);
if (response.status !== 200) {
  throw new Error(`terminal tab probe 실패: HTTP ${response.status}`);
}
const state = json(response.body);
if (state.windowCount !== 2 || !state.initialPaneRetained || !state.newPaneId) {
  throw new Error(`새 탭 또는 기존 pane 누락: ${response.body}`);
}
if (MARKER && (!state.ran || !state.newPaneReceivedMarker || state.initialPaneReceivedMarker)) {
  throw new Error(`새 pane의 명령 실행을 확인하지 못함: ${response.body}`);
}
output.createdPaneId = state.newPaneId;
