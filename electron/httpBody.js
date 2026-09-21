/**
 * hook 서버와 모바일 라우트가 함께 쓰는 HTTP 본문 읽기/쓰기.
 *
 * 본문 크기에 상한을 두는 이유는 `POST /api/mobile/pair`가 인증 앞단에 있기 때문이다.
 * 같은 네트워크의 아무 호스트나 끝나지 않는 본문을 흘려보내 Electron 메인 프로세스 힙을 채울 수 있어서,
 * 한도를 넘는 순간 소켓을 끊고 거절한다. hook 입력은 짧은 문자열 몇 개뿐이라 이 한도에 걸리지 않는다.
 */

const MAX_BODY_BYTES = 64 * 1024;

function readJsonBody(request) {
  return new Promise((resolve, reject) => {
    let body = "";
    let receivedBytes = 0;

    request.on("data", (chunk) => {
      receivedBytes += Buffer.byteLength(chunk);
      if (receivedBytes > MAX_BODY_BYTES) {
        request.destroy();
        reject(new Error("요청 본문이 너무 큽니다"));
        return;
      }
      body += chunk;
    });
    request.on("end", () => {
      try {
        resolve(body ? JSON.parse(body) : {});
      } catch (error) {
        reject(error);
      }
    });
    request.on("error", reject);
  });
}

function writeJson(response, statusCode, payload) {
  response.writeHead(statusCode, { "Content-Type": "application/json" });
  response.end(JSON.stringify(payload));
}

module.exports = { MAX_BODY_BYTES, readJsonBody, writeJson };
