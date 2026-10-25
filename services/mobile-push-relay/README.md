# KanVibe 모바일 푸시 중계 서버

데스크탑의 인증된 상태 알림 요청을 FCM HTTP v1으로 전달한다. Firebase 서비스 계정 비밀 키는 이 서버에만 둔다. Node.js 24에서 추가 패키지 설치 없이 실행할 수 있다.

## 실행 준비

1. 모바일 앱과 동일한 Firebase 프로젝트에서 Firebase Cloud Messaging API를 활성화한다. 서버용 서비스 계정에 해당 프로젝트의 Firebase Cloud Messaging API Admin 권한을 부여하고 JSON 키를 서버의 저장소 밖에 저장한다. iOS는 같은 Firebase 프로젝트에 APNs 인증 키도 등록해야 한다. [FCM 서버 인증 공식 문서](https://firebase.google.com/docs/cloud-messaging/send/v1-api)
2. [config.example.json](./config.example.json)을 저장소 밖의 설정 파일로 복사한다. 예시의 꺾쇠 문자열과 경로는 자리 표시자이며 실제 자격 증명이 아니다. `relayToken`을 충분히 긴 임의 비밀값으로 교체하고 `serviceAccountPath`에 서버의 실제 JSON 키 경로를 지정한다. 상대 경로는 설정 파일의 디렉터리를 기준으로 해석한다. 설정 파일과 키 파일은 서버 실행 사용자만 읽도록 제한한다.
3. 서버를 실행한다.

```sh
node services/mobile-push-relay/server.mjs --config /absolute/path/outside/repository/relay.json
```

서버는 설정된 IP와 포트에 바인딩한다. 기본 예시는 로컬 주소 `127.0.0.1:4890`이다. 환경변수나 셸 설정을 변경하지 않는다. 서비스 계정 JSON의 `project_id`, `client_email`, RSA `private_key`가 필요하며 `private_key_id`는 선택 사항이다.

외부 서버에서 운영할 때는 HTTPS 역방향 프록시 뒤에 배치하고, 요청 크기·접속 빈도 제한을 설정한다. 서비스 계정 키와 중계 비밀값을 공개 호스팅 파일이나 앱 번들에 넣지 않는다. 이 구현은 배포를 수행하지 않는다.

## 데스크탑 연결

데스크탑 KanVibe를 시작하는 프로세스에 아래 두 설정을 제공한다.

| 설정 | 값 |
| --- | --- |
| `KANVIBE_MOBILE_PUSH_RELAY_URL` | 전체 발송 URL, 예: `https://push.example.com/notifications` |
| `KANVIBE_MOBILE_PUSH_RELAY_TOKEN` | 중계 설정 파일의 `relayToken`과 동일한 값 |

로컬 개발은 `http://127.0.0.1:4890/notifications`를 사용할 수 있다. 중계 비밀값은 서버와 신뢰할 수 있는 데스크탑만 공유한다. 모바일에는 Firebase 서비스 계정 키나 중계 비밀값을 보내지 않는다. OAuth 토큰은 서버 메모리에서 만료 전에 갱신한다.

## HTTP 계약

`POST /notifications`는 `Authorization: Bearer <relayToken>`과 `Content-Type: application/json`을 요구한다. 인증을 확인한 뒤 본문을 읽는다.

```json
{
  "token": "<mobile-fcm-registration-token>",
  "notification": {
    "title": "KanVibe",
    "body": "검토할 태스크가 있습니다."
  },
  "data": {
    "type": "task-status-changed",
    "eventId": "<event-id>",
    "connectionId": "<saved-desktop-connection-id>",
    "taskId": "<task-id>",
    "title": "KanVibe",
    "body": "검토할 태스크가 있습니다.",
    "status": "review",
    "createdAt": "2026-10-23T00:00:00.000Z"
  }
}
```

상위 키와 `data` 키는 예시와 정확히 일치해야 하고, 모든 `data` 값은 문자열이어야 한다. `connectionId`, `taskId`, `eventId`를 그대로 보존하여 앱이 정확한 PC·태스크로 이동하고 중복 이벤트를 판별할 수 있게 한다. 페어링 인증 토큰은 포함하지 않는다.

요청 전체는 16 KiB 이하이고, FCM 전송 옵션을 포함한 JSON 크기는 대상 토큰을 제외하고 4096바이트 이하이다. UTF-8 바이트로 검사한다. 개별 제한은 푸시 토큰 4096바이트, 식별자 각 128바이트, 제목 512바이트, 본문 2048바이트, 상태·시각 각 64바이트다. 제목과 본문은 여러 플랫폼 필드에 중복 포함되므로 전체 제한이 먼저 적용될 수 있다. `createdAt`은 UTC ISO 8601 문자열을 사용한다.

| 응답 | 의미 |
| --- | --- |
| `200 {"success":true}` | FCM이 요청을 접수함. 실제 기기 도착 여부를 의미하지 않음 |
| `400` | 잘못된 JSON, 필드, 시각 또는 FCM 메시지 크기 |
| `401 {"error":"unauthorized"}` | 중계 비밀값 불일치 |
| `413` | 요청 전체 크기 초과 |
| `415` | JSON Content-Type이 아님 |
| `410 {"error":"unregistered"}` | FCM `UNREGISTERED`. 데스크탑은 해당 푸시 토큰을 해제 |
| `502 {"error":"push_unavailable"}` | OAuth·FCM 오류 또는 타임아웃. 등록 토큰을 유지 |

서버는 공급자의 원문 오류, 등록 토큰, OAuth 토큰, 비밀 키를 응답하거나 로그에 기록하지 않는다. 일반 FCM 404는 `UNREGISTERED`로 간주하지 않는다. [FCM 오류 코드 공식 문서](https://firebase.google.com/docs/cloud-messaging/error-codes)

## 시스템 알림과 클릭 데이터

공통 `notification`과 `data`를 함께 보낸다. Android는 `HIGH` 우선순위와 `kanvibe_task_status` 채널을 사용한다. iOS는 APNs `alert` 푸시, 우선순위 `10`, 기본 알림음을 사용한다. 앱에서 채널 생성과 알림 권한 허용이 필요하다. [FCM 메시지 계약 공식 문서](https://firebase.google.com/docs/reference/fcm/rest/v1/projects.messages)

같은 PC·태스크는 SHA-256으로 만든 64바이트 병합 ID를 공유한다. Android 알림 `tag`와 APNs `apns-collapse-id`에 적용하여 같은 태스크의 알림을 갱신한다. 알림을 누른 앱은 `data.connectionId`와 `data.taskId`로 이동하고, API에서 최신 상태를 조회한다. 백그라운드에서 화면 갱신 코드가 항상 실행되는 것을 전제로 하지 않는다.

## 검증

```sh
node --test services/mobile-push-relay/relay.node-test.mjs
```

테스트는 임시 로컬 HTTP 서버에 실제 요청을 보내고 Google OAuth·FCM 네트워크 경계만 대체한다. 테스트용 RSA 키는 실행 중에 생성하며 실제 프로젝트나 기기에 연결하지 않는다. 인증 거절, RS256 서명·OAuth scope·캐시·갱신, FCM 메시지·클릭 식별자, 오류 매핑, 크기 제한과 타임아웃을 검사한다.

실제 잠금 화면 수신과 클릭 이동은 Firebase 프로젝트 설정, 서버 운영 환경, APNs 설정 및 실기기에서 별도로 검증해야 한다.
