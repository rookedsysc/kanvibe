// 개발 실행의 데스크탑에서 페어링 코드를 받는다. 이 경로는 `pnpm dev`와 이 머신에서 온 요청에만 열려 있다.
const response = http.post(`${DESKTOP_API_FROM_HOST}/api/mobile/dev/pairing-code`, { body: "" });
if (response.status !== 200) {
  throw new Error(`개발용 페어링 코드를 받지 못했습니다: HTTP ${response.status}. pnpm dev로 띄운 데스크탑인지 확인하세요.`);
}
output.pairingCode = json(response.body).code;
