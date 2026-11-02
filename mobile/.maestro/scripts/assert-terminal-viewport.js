// 실제 tmux 격자를 확인한다. 화면의 모드 문구만으로 resize 성공을 판정하지 않는다.
const response = http.get(`${PROBE_API_FROM_HOST}/terminal-state?minColumns=${MIN_COLUMNS}&maxColumns=${MAX_COLUMNS}`);
if (response.status !== 200) {
  throw new Error(`terminal viewport probe 실패: HTTP ${response.status}`);
}
const state = json(response.body);
const minimum = Number(MIN_COLUMNS);
const maximum = Number(MAX_COLUMNS);
if (state.columns < minimum || state.columns > maximum || state.rows < 1) {
  throw new Error(`격자 범위 불일치: ${state.columns}열 × ${state.rows}행 (기대 ${minimum}..${maximum}열)`);
}
output.terminalViewport = state;
