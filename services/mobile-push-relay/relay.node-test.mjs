import assert from "node:assert/strict";
import { generateKeyPairSync, verify } from "node:crypto";
import { once } from "node:events";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { request } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { createPushRelayServer, loadRelayConfiguration } from "./server.mjs";
import { createFcmProvider } from "./fcm-provider.mjs";

const relayToken = "test-only-relay-token-not-a-production-secret";
const { privateKey, publicKey } = generateKeyPairSync("rsa", { modulusLength: 2048 });
const serviceAccount = {
  project_id: "kanvibe-relay-test",
  client_email: "test-only@kanvibe-relay-test.iam.gserviceaccount.com",
  private_key: privateKey.export({ type: "pkcs8", format: "pem" }),
  private_key_id: "test-key-id",
};

function notification(overrides = {}) {
  return {
    token: "test-only-registration-token",
    notification: { title: "KanVibe", body: "리뷰할 태스크가 있습니다." },
    data: {
      type: "task-status-changed",
      eventId: "event-123",
      connectionId: "desktop-pc-456",
      taskId: "task-789",
      title: "KanVibe",
      body: "리뷰할 태스크가 있습니다.",
      status: "review",
      createdAt: "2026-10-23T00:00:00.000Z",
    },
    ...overrides,
  };
}

async function startRelay(context, fetchImpl, options = {}) {
  const provider = createFcmProvider({ serviceAccount, fetchImpl, ...options });
  const server = createPushRelayServer({ relayToken, provider });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  context.after(() => new Promise((resolve) => server.close(resolve)));
  return `http://127.0.0.1:${server.address().port}/notifications`;
}

function send(url, payload, headers = {}) {
  return fetch(url, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Authorization: `Bearer ${relayToken}`,
      ...headers,
    },
    body: typeof payload === "string" ? payload : JSON.stringify(payload),
  });
}

test("인증 실패 시 잘못된 JSON도 파싱하거나 FCM에 전달하지 않는다", async (context) => {
  let upstreamCalls = 0;
  const url = await startRelay(context, async () => {
    upstreamCalls += 1;
    throw new Error("External network must not be reached");
  });

  const response = await send(url, "{invalid JSON", { Authorization: "Bearer wrong-secret" });

  assert.equal(response.status, 401);
  assert.deepEqual(await response.json(), { error: "unauthorized" });
  assert.equal(upstreamCalls, 0);
});

test("실제 HTTP 요청이 식별자를 보존한 시스템 알림과 서명된 OAuth 요청을 생성한다", async (context) => {
  const calls = [];
  const now = Date.parse("2026-10-23T00:00:00.000Z");
  const url = await startRelay(context, async (url, options) => {
    calls.push({ url, options });
    return calls.length === 1
      ? Response.json({ access_token: "test-only-access-token", expires_in: 3600, token_type: "Bearer" })
      : Response.json({ name: "projects/kanvibe-relay-test/messages/message-1" });
  }, { now: () => now });
  const payload = notification();

  const response = await send(url, payload);

  assert.equal(response.status, 200);
  assert.deepEqual(await response.json(), { success: true });
  assert.equal(calls[0].url, "https://oauth2.googleapis.com/token");
  assert.equal(calls[0].options.headers["Content-Type"], "application/x-www-form-urlencoded");
  const oauthRequest = new URLSearchParams(calls[0].options.body);
  assert.equal(oauthRequest.get("grant_type"), "urn:ietf:params:oauth:grant-type:jwt-bearer");
  const [header, claims, signature] = oauthRequest.get("assertion").split(".");
  assert.deepEqual(JSON.parse(Buffer.from(header, "base64url")), {
    alg: "RS256", typ: "JWT", kid: "test-key-id",
  });
  assert.deepEqual(JSON.parse(Buffer.from(claims, "base64url")), {
    iss: serviceAccount.client_email,
    scope: "https://www.googleapis.com/auth/firebase.messaging",
    aud: "https://oauth2.googleapis.com/token",
    iat: now / 1000,
    exp: now / 1000 + 3600,
  });
  assert.equal(verify("RSA-SHA256", Buffer.from(`${header}.${claims}`), publicKey,
    Buffer.from(signature, "base64url")), true);
  assert.equal(calls[1].url, "https://fcm.googleapis.com/v1/projects/kanvibe-relay-test/messages:send");
  assert.equal(calls[1].options.headers.Authorization, "Bearer test-only-access-token");
  const { message } = JSON.parse(calls[1].options.body);
  assert.equal(message.token, payload.token);
  assert.deepEqual(message.notification, payload.notification);
  assert.deepEqual(message.data, payload.data);
  assert.equal(message.android.priority, "HIGH");
  assert.equal(message.android.notification.channel_id, "kanvibe_task_status");
  assert.equal(message.android.notification.sound, "default");
  assert.equal(message.apns.headers["apns-push-type"], "alert");
  assert.equal(message.apns.headers["apns-priority"], "10");
  assert.deepEqual(message.apns.payload.aps.alert, payload.notification);
  assert.equal(message.apns.payload.aps.sound, "default");
  assert.match(message.apns.headers["apns-collapse-id"], /^[a-f0-9]{64}$/);
  assert.equal(message.android.collapse_key, message.apns.headers["apns-collapse-id"]);
  assert.equal(message.android.notification.tag, message.android.collapse_key);
});

test("같은 PC와 태스크의 알림만 병합하고 OAuth 토큰을 만료 전에 갱신한다", async (context) => {
  const messages = [];
  let oauthCalls = 0;
  let currentTime = Date.parse("2026-10-23T00:00:00.000Z");
  const url = await startRelay(context, async (url, options) => {
    if (url === "https://oauth2.googleapis.com/token") {
      oauthCalls += 1;
      return Response.json({ access_token: `access-${oauthCalls}`, expires_in: 3600 });
    }
    messages.push(JSON.parse(options.body).message);
    return Response.json({ name: "projects/kanvibe-relay-test/messages/message-2" });
  }, { now: () => currentTime });

  assert.equal((await send(url, notification())).status, 200);
  const newer = notification();
  newer.data.eventId = "event-124";
  assert.equal((await send(url, newer)).status, 200);
  assert.equal(oauthCalls, 1);
  assert.equal(messages[0].android.collapse_key, messages[1].android.collapse_key);
  currentTime += 3550 * 1000;
  const otherPc = notification();
  otherPc.data.connectionId = "another-pc";
  assert.equal((await send(url, otherPc)).status, 200);
  assert.equal(oauthCalls, 2);
  assert.notEqual(messages[0].android.collapse_key, messages[2].android.collapse_key);
});

test("UNREGISTERED FCM 토큰만 410으로 반환하고 공급자 비밀 내용을 노출하지 않는다", async (context) => {
  const url = await startRelay(context, async (url) => {
    return url === "https://oauth2.googleapis.com/token"
      ? Response.json({ access_token: "access", expires_in: 3600 })
      : Response.json({ error: { message: "private token detail", status: "NOT_FOUND", details: [
        { "@type": "type.googleapis.com/google.firebase.fcm.v1.FcmError", errorCode: "UNREGISTERED" },
      ] } }, { status: 404 });
  });

  const response = await send(url, notification());

  assert.equal(response.status, 410);
  assert.deepEqual(await response.json(), { error: "unregistered" });
});

test("일반 FCM 404는 기기 토큰을 삭제하게 하는 410으로 바꾸지 않는다", async (context) => {
  const url = await startRelay(context, async (url) => {
    return url === "https://oauth2.googleapis.com/token"
      ? Response.json({ access_token: "access", expires_in: 3600 })
      : Response.json({ error: { status: "NOT_FOUND", message: "private project detail" } }, { status: 404 });
  });

  const response = await send(url, notification());

  assert.equal(response.status, 502);
  assert.deepEqual(await response.json(), { error: "push_unavailable" });
});

test("공급자 일시 오류는 안전한 502를 반환한다", async (context) => {
  const url = await startRelay(context, async (url) => {
    return url === "https://oauth2.googleapis.com/token"
      ? Response.json({ access_token: "access", expires_in: 3600 })
      : new Response("private registration token and upstream stack", { status: 503 });
  });

  const response = await send(url, notification());

  assert.equal(response.status, 502);
  assert.deepEqual(await response.json(), { error: "push_unavailable" });
});

test("OAuth 발급 실패는 FCM을 호출하지 않고 이후 정상 발급을 재시도한다", async (context) => {
  let oauthCalls = 0;
  let fcmCalls = 0;
  const url = await startRelay(context, async (url) => {
    if (url === "https://oauth2.googleapis.com/token") {
      oauthCalls += 1;
      return oauthCalls === 1
        ? Response.json({ error: "invalid_grant", error_description: "private key detail" }, { status: 400 })
        : Response.json({ access_token: "access", expires_in: 3600 });
    }
    fcmCalls += 1;
    return Response.json({ name: "projects/kanvibe-relay-test/messages/message-3" });
  });

  const failed = await send(url, notification());
  assert.equal(failed.status, 502);
  assert.deepEqual(await failed.json(), { error: "push_unavailable" });
  assert.equal(fcmCalls, 0);
  assert.equal((await send(url, notification())).status, 200);
  assert.equal(oauthCalls, 2);
  assert.equal(fcmCalls, 1);
});

test("동시에 도착한 요청은 진행 중인 OAuth 발급을 공유한다", async (context) => {
  let oauthCalls = 0;
  const started = Promise.withResolvers();
  const release = Promise.withResolvers();
  const url = await startRelay(context, async (url) => {
    if (url === "https://oauth2.googleapis.com/token") {
      oauthCalls += 1;
      started.resolve();
      await release.promise;
      return Response.json({ access_token: "access", expires_in: 3600 });
    }
    return Response.json({ name: "projects/kanvibe-relay-test/messages/message-4" });
  });

  const first = send(url, notification());
  await started.promise;
  const second = send(url, notification());
  release.resolve();
  const responses = await Promise.all([first, second]);

  assert.deepEqual(responses.map((response) => response.status), [200, 200]);
  assert.equal(oauthCalls, 1);
});

test("허용한 문자열 필드 외의 임의 FCM 옵션은 거절한다", async (context) => {
  let upstreamCalls = 0;
  const url = await startRelay(context, async () => { upstreamCalls += 1; });
  const payload = notification();
  payload.data.taskId = 789;
  payload.data.extra = "unsupported";
  payload.topic = "all-devices";

  const response = await send(url, payload);

  assert.equal(response.status, 400);
  assert.deepEqual(await response.json(), { error: "invalid_notification" });
  assert.equal(upstreamCalls, 0);
});

test("알림 식별자 또는 ISO 시각이 없으면 발송하지 않는다", async (context) => {
  let upstreamCalls = 0;
  const url = await startRelay(context, async () => { upstreamCalls += 1; });
  const payload = notification();
  payload.data.connectionId = "";
  payload.data.createdAt = "not-a-date";

  const response = await send(url, payload);

  assert.equal(response.status, 400);
  assert.equal(upstreamCalls, 0);
});

test("최대 요청 크기를 넘으면 인증 후에도 발송하지 않는다", async (context) => {
  let upstreamCalls = 0;
  const url = await startRelay(context, async () => { upstreamCalls += 1; });

  const response = await send(url, "x".repeat(16 * 1024 + 1));

  assert.equal(response.status, 413);
  assert.deepEqual(await response.json(), { error: "request_too_large" });
  assert.equal(upstreamCalls, 0);
});

test("FCM 메시지 크기를 넘는 다국어 본문은 공급자 호출 전에 거절한다", async (context) => {
  let upstreamCalls = 0;
  const url = await startRelay(context, async () => { upstreamCalls += 1; });
  const payload = notification();
  payload.data.body = "한".repeat(500);
  payload.notification.body = payload.data.body;

  const response = await send(url, payload);

  assert.equal(response.status, 400);
  assert.equal(upstreamCalls, 0);
});

test("문자열이 아닌 태스크 ID는 다른 필드가 정상이더라도 거절한다", async (context) => {
  let upstreamCalls = 0;
  const url = await startRelay(context, async () => { upstreamCalls += 1; });
  const payload = notification();
  payload.data.taskId = 789;

  const response = await send(url, payload);

  assert.equal(response.status, 400);
  assert.equal(upstreamCalls, 0);
});

test("인증되어도 JSON 외의 Content-Type은 거절한다", async (context) => {
  let upstreamCalls = 0;
  const url = await startRelay(context, async () => { upstreamCalls += 1; });

  const response = await send(url, notification(), { "Content-Type": "text/plain" });

  assert.equal(response.status, 415);
  assert.deepEqual(await response.json(), { error: "json_required" });
  assert.equal(upstreamCalls, 0);
});

test("인증된 잘못된 JSON은 안전한 400으로 반환한다", async (context) => {
  let upstreamCalls = 0;
  const url = await startRelay(context, async () => { upstreamCalls += 1; });

  const response = await send(url, "{invalid JSON");

  assert.equal(response.status, 400);
  assert.deepEqual(await response.json(), { error: "invalid_json" });
  assert.equal(upstreamCalls, 0);
});

test("Content-Length 없는 청크 요청도 크기 제한을 적용한다", async (context) => {
  let upstreamCalls = 0;
  const url = await startRelay(context, async () => { upstreamCalls += 1; });

  const status = await new Promise((resolve, reject) => {
    const outgoing = request(url, {
      method: "POST",
      headers: { Authorization: `Bearer ${relayToken}`, "Content-Type": "application/json" },
    }, (response) => {
      response.resume();
      response.once("end", () => resolve(response.statusCode));
    });
    outgoing.once("error", reject);
    outgoing.write("x".repeat(8 * 1024));
    outgoing.end("x".repeat(8 * 1024 + 1));
  });

  assert.equal(status, 413);
  assert.equal(upstreamCalls, 0);
});

test("외부 요청이 응답하지 않으면 제한 시간에 발송 실패로 반환한다", async (context) => {
  const url = await startRelay(context, async (_url, options) => {
    await once(options.signal, "abort");
    throw options.signal.reason;
  }, { requestTimeoutMs: 10 });

  const response = await send(url, notification());

  assert.equal(response.status, 502);
  assert.deepEqual(await response.json(), { error: "push_unavailable" });
});

test("명시한 외부 설정 파일을 기준으로 서비스 계정 상대 경로를 읽는다", (context) => {
  const directory = mkdtempSync(join(tmpdir(), "kanvibe-push-relay-test-"));
  context.after(() => rmSync(directory, { recursive: true, force: true }));
  const accountPath = join(directory, "test-only-service-account.json");
  const configPath = join(directory, "relay.json");
  writeFileSync(accountPath, JSON.stringify(serviceAccount), { mode: 0o600 });
  writeFileSync(configPath, JSON.stringify({
    host: "127.0.0.1", port: 4890, relayToken, serviceAccountPath: "test-only-service-account.json",
  }), { mode: 0o600 });

  const config = loadRelayConfiguration(configPath);

  assert.deepEqual(config, { host: "127.0.0.1", port: 4890, relayToken, serviceAccount });
});
