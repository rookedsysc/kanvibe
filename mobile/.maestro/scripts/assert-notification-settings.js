// 기기에서 저장한 값을 같은 PC의 인증 API로 다시 확인한다. 인증값은 이 호출 안에서만 쓴다.
const codeResponse = http.post(`${DESKTOP_API_FROM_HOST}/api/mobile/dev/pairing-code`, { body: "" });
if (codeResponse.status !== 200) throw new Error('페어링 코드 조회 실패');
const pair = http.post(`${DESKTOP_API_FROM_HOST}/api/mobile/pair`, {
  body: JSON.stringify({ code: json(codeResponse.body).code, deviceName: 'Settings QA observer' }),
  headers: { 'Content-Type': 'application/json' },
});
if (pair.status !== 200) throw new Error('QA 관측 기기 연결 실패');
const headers = { Authorization: `Bearer ${json(pair.body).token}` };
const response = http.get(`${DESKTOP_API_FROM_HOST}/api/mobile/notification-settings`, { headers });
http.delete(`${DESKTOP_API_FROM_HOST}/api/mobile/device`, { headers });
if (response.status !== 200) throw new Error(`설정 조회 실패: ${response.status}`);
const settings = json(response.body).settings;
if (settings.isEnabled !== (EXPECT_ENABLED === 'true') ||
    settings.enabledStatuses.includes('todo') !== (EXPECT_TODO === 'true')) {
  throw new Error(`저장된 설정 불일치: ${JSON.stringify(settings)}`);
}
