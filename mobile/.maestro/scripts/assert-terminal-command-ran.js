// 기기에서 친 명령을 데스크탑 셸이 실행했는지 묻는다. probe가 생길 때까지 기다려 주므로 여기서는 한 번만 부른다.
const response = http.get(`${PROBE_API_FROM_HOST}/command-ran?marker=MAESTRO_INPUT_OK&timeoutMs=15000`);
if (response.status !== 200) {
  throw new Error(`terminal probe에 닿지 못했습니다: HTTP ${response.status}`);
}
output.terminalCommandRan = json(response.body).ran;
