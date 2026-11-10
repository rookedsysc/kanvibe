const { writeJson } = require("./httpBody.js");

/**
 * 웹페이지가 보낸 요청을 거부한다. 거부했으면 true를 돌려준다.
 *
 * 브라우저는 다른 출처로 보내는 POST에 항상 Origin을 붙이고 페이지가 이를 지울 수 없다.
 * hook 스크립트와 모바일 앱은 브라우저가 아니라 이 헤더를 보내지 않으므로, 있으면 사용자가 방문한 웹페이지의 요청으로 본다.
 * WebSocket은 이 경로를 지나지 않고 upgrade에서 Bearer 토큰으로 따로 인증한다.
 */
function rejectBrowserOriginRequest(request, response) {
  if (request.headers.origin === undefined) {
    return false;
  }

  writeJson(response, 403, { success: false, error: "브라우저 요청은 허용하지 않습니다" });
  return true;
}

module.exports = { rejectBrowserOriginRequest };
