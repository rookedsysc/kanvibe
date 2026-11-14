# 원격 호스트 연결과 터미널 운영

호스트별 공용 연결은 원격 명령·hook·터미널을 지속 SSH 연결로 전달하고, 원격 daemon이 연결과 별개로 실행 상태를 유지하는 구조입니다. daemon은 화면 뒤에서 실행되는 서버 프로세스이며, PTY는 셸과 터미널 프로그램에 화면 크기·입출력을 제공하는 가상 터미널입니다.

이 가이드는 PR #470 개발 후보의 운영 계약을 설명합니다. 배포된 앱의 지원 범위는 해당 앱 빌드와 호스트가 응답한 기능 목록으로 확인해야 합니다. 원격 서버의 생존, hook 이벤트 보관, 모바일 접근 권한은 서로 다른 보장입니다.

## 연결을 끊어도 남는 것

namespace는 같은 원격 계정에서 `dev`와 `prod`의 서버·소켓·상태를 분리하는 이름입니다. 데스크톱 연결은 SSH의 유효 사용자·호스트·포트·인증·경유 설정과 namespace로 식별합니다. `%n`이나 `%k`가 남는 설정은 원래 SSH 별칭도 구분에 포함합니다.

| 대상 | 소유자와 식별 | 연결 종료 뒤 |
| --- | --- | --- |
| 공용 SSH 연결 | 데스크톱의 호스트 연결 모듈 | 닫히며 다음 접속에서 다시 연결 |
| 원격 daemon | 원격 OS 사용자·namespace | SSH나 앱 종료 뒤에도 유지. OS 재부팅·명시 중지에는 종료 |
| 일반 terminal의 PTY | daemon, 발급된 session ID와 데스크톱 client ID | detach 후 같은 소유 세션에 재접속 |
| tmux·zellij 접속용 PTY | daemon | detach 후 유지. tmux·zellij 자체 서버와 별도 수명 |
| Herdr 세션 | 기존 Herdr 서버 | KanVibe 접속 해제로 Herdr 서버·다른 사용자의 세션을 종료하지 않음 |
| 모바일 grant | 데스크톱 client·기기·태스크별 접근 허가 | 연결 해제·기기 해제 등 기존 권한 회수 정책 적용 |
| 보드와 태스크 상태 | 기존 앱 DB·태스크 상태 파일 | daemon에 별도 태스크 DB를 만들지 않음 |

같은 원격 사용자와 namespace에 연결한 여러 데스크톱은 daemon을 공유하되 client ID로 PTY 소유와 hook 구독을 구분합니다. 한 client가 다른 client의 PTY를 조회하거나 닫을 수 없습니다. 동일 OS 계정의 모든 프로세스가 서로 격리된다는 보장은 제공하지 않습니다.

서버가 살아 있어도 모바일 접근 권한이 계속 유효한 것은 아닙니다. 한 기기의 grant를 회수해도 다른 기기의 연결과 Herdr controller는 유지하며, 직접 WSS 연결·중계·인증서 pin 검증은 기존 계약을 따릅니다.

## 터미널을 닫을 때와 화면만 떠날 때

일반 `terminal`, `tmux`, `zellij`의 새 경로는 daemon의 PTY에 붙습니다. Herdr는 기존 서버의 native 연결을 재사용합니다.

| 동작 | 일반 terminal | tmux·zellij | Herdr |
| --- | --- | --- | --- |
| 창·화면·앱을 떠나거나 네트워크 단절 | 접속만 해제하고 원격 PTY 유지 | 접속용 PTY와 multiplexer 세션 유지 | KanVibe 접속 해제, Herdr 세션 유지 |
| 동일 client가 다시 열기 | 발급된 세션을 찾아 현재 화면부터 구독 | 같은 접속용 PTY에 다시 연결 | 기존 Herdr 세션에 다시 연결 |
| 탭 명시 닫기 | 해당 원격 PTY 종료 | 마지막 탭이면 창만 닫기. 그 외 해당 multiplexer 탭 닫기 | 마지막 탭이면 창만 닫기. 그 외 해당 Herdr 탭 닫기 |
| daemon 명시 중지 | daemon 소유 PTY 모두 종료 | daemon 접속용 PTY 종료. 외부 multiplexer 서버의 수명은 별도 | daemon 종료를 Herdr 서버 중지로 확대하지 않음 |

태스크 삭제나 명시적인 세션 종료는 화면 닫기와 다른 정리 경로이며 연결된 세션 자체를 끝낼 수 있습니다.

재접속 화면은 제한된 현재 화면과 최근 스크롤 내용의 snapshot입니다. snapshot은 재접속 때 보내는 화면 사본이며 전체 실행 로그의 영구 보관이 아닙니다. 현재 원격 PTY 화면은 스크롤 200줄을 유지합니다.

입력은 연결이 끊겼을 때 자동으로 다시 보내지 않습니다. detach 시 아직 쓰지 않은 입력은 버리며, 이미 커널 쓰기를 시작한 입력은 실행 여부가 불명확할 수 있습니다. 같은 명령이나 붙여넣기를 자동 반복하면 부수효과가 중복될 수 있으므로 재접속 후 실제 화면과 명령 결과를 확인합니다.

출력 ACK는 수신한 바이트를 확인하는 응답입니다. 접속별 미확인 출력이 1 MiB를 넘으면 느린 접속을 떼어 내고 현재 화면으로 재접속하도록 합니다. 화면 처리 적체 때에는 PTY 읽기를 잠시 멈춥니다. 입력은 커널 쓰기 완료 뒤 다음 chunk를 허용하며, transport는 32 KiB chunk·1 MiB frame·8 MiB buffer 상한을 둡니다. 큰 출력이나 느린 수신자를 위해 무제한 메모리를 쓰지 않습니다.

## 원격 플랫폼과 검증 범위

원격 PTY 빌드에는 다음 OS·CPU별 파일이 포함됩니다. 파일이 있다는 사실과 실제 호스트에서 실행·재접속을 검증했다는 사실은 구분해야 합니다. 특정 커밋의 실행 환경과 결과는 [PR #470의 QA 코멘트](https://github.com/rookedsysc/kanvibe/pull/470)에서 확인합니다.

| 원격 플랫폼 | PTY 파일 제공 | 실행 검증 기준 |
| --- | --- | --- |
| macOS arm64 | 제공 | Node18·Electron의 native 로딩·입출력·재접속 결과는 PR QA 기록 참조 |
| macOS x64 | 제공 | 이 후보에서 실제 실행 미검증 |
| Linux x64 | 제공 | 일반 PTY·tmux·zellij의 실제 SSH 및 Node18·Electron 실행 결과는 PR QA 기록 참조 |
| Linux arm64 | 제공 | 이 후보에서 실제 실행 미검증 |
| Linux musl 계열 | 보장하지 않음 | Node-API 버전만으로 libc 호환성을 판정하지 않음 |
| Windows | 새 daemon·원격 PTY 미지원 | 기존 경로 유지. ConPTY 이관 완료로 보지 않음 |

명령·hook·모바일 helper의 기존 Linux SSH 검증을 일반 PTY 전체의 검증으로 대신하지 않습니다. 실제 Android/iOS 앱의 화면 여정도 서버 프로토콜 검증과 별개입니다.

일반 terminal은 호스트 셸, tmux·zellij는 해당 호스트의 기존 실행 파일이 필요합니다. 새 PTY는 multiplexer 서버를 대체하지 않으며 사용자가 실행 중인 서버·세션을 강제로 재시작하지 않습니다.

## hook 이벤트는 저장 성공과 앱 반영을 구분한다

원격 hook는 namespace의 안정 소켓에 요청을 보내고 데스크톱 client ID로 전달 대상을 정합니다. 영속 journal은 끊김 중 이벤트를 보관하는 파일이며 `POST /api/hooks/status`만 저장합니다. Task CLI의 태스크 생성·삭제·등록 요청과 터미널 입력·원격 명령은 journal에 넣어 나중에 실행하지 않습니다.

| 응답·상태 | 의미 | 후속 동작 |
| --- | --- | --- |
| HTTP `202`와 `queued: true`, `eventId` | 디스크에 저장했으나 해당 데스크톱이 연결·구독하지 않음 | 같은 client가 재접속·구독하면 전달. 보드 반영 완료 응답이 아님 |
| 데스크톱의 HTTP `200` 등 성공 응답 (`success: false` 제외) | 데스크톱 처리 결과를 받음 | journal의 해당 이벤트를 확인 처리 |
| `delivery: ignored` | 같은 상태·오래된 상태·조건부 갱신 경합 등으로 변경 불필요 | 이벤트를 확인 처리하고 다음 이벤트 진행 |
| 데스크톱 처리의 HTTP `4xx` | 잘못된 입력·허용하지 않은 host/task 등 영구 거부 | 거부로 확인 처리. 같은 이벤트 재전달 중지 |
| HTTP `5xx`, 처리 제한시간 초과, 단절 | 일시 실패 또는 결과 불명 | 저장된 이벤트를 유지하고 재전달 |
| 저장 시 HTTP `503` | 용량·동시 요청 한도·파일 쓰기 실패 | 저장 성공으로 보지 않음. 이미 보관된 이벤트를 밀어내지 않음 |
| HTTP `400`·`413` | 잘못된 형식 또는 본문 크기 초과 | 입력을 고쳐야 하며 저장 성공으로 보지 않음 |

서버가 발급한 이벤트 ID와 sequence로 client별 먼저 들어온 이벤트부터 하나씩 전달합니다. 성공 또는 영구 거부로 확인할 때까지 같은 client의 다음 이벤트는 기다립니다. 다른 client의 전달은 별도로 진행합니다. 일시 실패는 현재 구현에서 5초 뒤 다시 시도하며, 확인 응답을 잃으면 이미 처리한 이벤트도 재전달될 수 있습니다.

재전달된 상태 hook는 host/task 범위를 다시 검사하고 원격의 현재 태스크 상태 파일과 대조합니다. 현재 상태와 다른 과거 이벤트, 이미 반영한 상태, DB의 조건부 갱신에서 뒤처진 이벤트는 `ignored`로 처리해 과거 상태를 다시 쓰지 않습니다. 재전달로 원격 상태 파일을 덮어쓰지 않으며 실제 변경이 적용된 경우에만 후속 갱신을 알립니다. 현재 상태를 읽을 수 없으면 `503`으로 재시도합니다. 무제한 보관이나 한 번만 전달되는 전송을 보장하지 않습니다.

### 보관 한도와 저장 위치

| 항목 | 현재 한도 |
| --- | --- |
| 보관 기간 | 7일 |
| namespace 전체 이벤트 수 | 4,096개 |
| namespace journal 파일 | 16 MiB |
| 요청 본문 | 512 KiB |
| 동시 처리 중인 요청 | 128개 |

만료 이벤트는 journal 접근 시 정리하고 만료 횟수를 보존합니다. 전체 파일 한도에는 JSON·base64 등 저장 표현이 포함되므로 본문 합계가 16 MiB가 되기 전에 새 저장을 거절할 수 있습니다. 새 이벤트를 받기 위해 아직 유효한 이벤트를 자동 삭제하지 않습니다.

기본 저장 파일은 원격 사용자 홈의 `.local/state/kanvibe/host/<namespace>/hook-journal-v1.json`입니다. KanVibe가 소유하는 state·namespace 디렉터리는 `0700`, journal 파일은 `0600`으로 만들고 소유 사용자·symlink·권한을 검사합니다. 더 상위의 기존 `.local`·`state` 경로는 다른 사용자의 쓰기가 허용되면 거부하며 임의로 권한을 고치지 않습니다.

파일과 상위 디렉터리를 동기화하고 원자적으로 교체한 뒤 저장 성공을 응답합니다. 서버 재시작 후에는 같은 state 파일을 다시 읽습니다. 파일 손상·소유권 불일치는 빈 journal로 덮지 않고 시작을 실패시킵니다. 파일을 수동으로 삭제하면 미전달 이벤트도 사라집니다.

실행용 파일과 소켓은 `/tmp/kv-<uid>/<namespace>/`에 둡니다. `rpc-v1.sock`은 공용 연결, `hooks-v1.sock`은 hook·Task CLI의 안정 입구입니다. 이 디렉터리는 `0700`, 소켓은 `0600`입니다. OS 재부팅 뒤 `/tmp`가 지워져도 journal은 별도 state 위치에 남지만 daemon과 PTY 자체는 재부팅을 넘어 생존하지 않습니다.

## diff 갱신은 호스트별로 제한한다

보드는 저장된 diff 집계를 먼저 반환하고 백그라운드에서 갱신합니다. 모든 호스트를 묶던 전역 2개 제한은 호스트별 2개로 나누며, 로컬 작업은 별도 제한을 씁니다. 한 호스트의 느린 요청이나 metadata 조회가 다른 호스트·로컬 작업을 막지 않습니다.

같은 태스크의 진행·대기 요청은 합치고, 사용자가 상세 화면을 열면 그 태스크를 백그라운드 대기에서 꺼내 먼저 읽습니다. 실패한 조회는 저장된 집계를 0으로 덮지 않습니다. 호스트 식별은 태스크의 기존 SSH 별칭을 따르며, 서로 다른 별칭을 같은 endpoint로 합치는 별도 diff 정책은 추가하지 않습니다. 호스트별 2개는 기존 상한을 분리한 값이며 모든 호스트의 최적 처리량을 뜻하지 않습니다.

## 서버를 확인하고 업그레이드한다

현재 일반 사용자를 위한 `kanvibe host status`, `doctor`, `stop`, `upgrade` CLI는 없습니다. 아래 이름은 공용 연결의 내부 제어 메시지이며 셸 명령으로 실행할 수 없습니다.

| 내부 응답 | 관측할 내용 |
| --- | --- |
| 접속 시 `ready` | protocol version, namespace, instance ID, PID, UID, build hash, 기능 목록 |
| `status` 응답의 `journal` | `pending`, `bytes`, `expired`, `rejected`, `ignored`, `activeRequests` |
| `pty.list` 응답 | 해당 client가 소유한 session ID·key·PID |
| `stop` 요청 | 해당 daemon 중지. daemon 소유 PTY 모두에 영향 |

`pending`은 남은 이벤트 수, `bytes`는 journal의 저장 표현 크기입니다. `expired`·`rejected`·`ignored`는 상태 파일에 보관하는 누적 횟수이며 현재 연결의 일시적인 수치가 아닙니다. `rejected`에는 용량으로 거절한 저장과 영구 거부된 전달이 포함됩니다. 서버 응답·오류 로그와 정확한 state 파일을 통해 조사하며, 전용 상태 표시 UI가 있다고 가정하지 않습니다.

원격 서버는 Node18 이상을 요구합니다. 로컬 desktop의 `node-pty@1.1.0` 의존성은 유지하고, 원격 전용 파일은 고정된 `node-pty@1.2.0-beta.14`에서 빌드합니다. 빌드 시 공식 npm archive의 SHA-512를 확인하고 파일별 SHA-256 manifest를 만듭니다. 원격에는 해당 OS·CPU 파일만 보내며 사용자 호스트에서 npm 설치·컴파일을 하지 않습니다.

원격 시작 전 OS·CPU·Node-API 버전과 파일 hash·경로·소유권을 검사하고 native 모듈을 실제로 로딩합니다. Node-API 조건을 만족해도 libc나 시스템 요구사항 때문에 로딩이 실패할 수 있습니다. beta 버전을 자동 추종하지 않으며 버전을 바꾸면 native·입력 ACK 경계를 다시 검증해야 합니다.

서버 build가 현재 앱과 다르면 기존 daemon을 자동 교체하거나 강제 종료하지 않습니다. 인증·host key·daemon protocol/build·무결성 오류를 기존 SSH PTY로 조용히 우회하지도 않습니다.

업그레이드와 되돌리기는 다음 순서로 운영자가 명시적으로 수행합니다.

1. 사용 중인 원격 사용자·namespace·daemon PID·instance ID·build hash와 연결 중인 client를 확인합니다. PID 파일만으로 실행 대상을 확정하지 않습니다.
2. 해당 daemon의 모든 client에서 작업을 저장하고 유지할 세션을 정리합니다. daemon 중지는 일반 terminal PTY를 끝내며, 실행 중인 명령도 연결 종료의 영향을 받습니다.
3. 호환 연결에서는 내부 `stop` 요청으로 중지할 수 있습니다. build 불일치로 연결이 거부되는 경우에는 원격 OS의 프로세스 정보에서 사용자·실행 파일 경로·namespace·build 인자를 대조한 뒤 그 daemon에만 `SIGTERM`을 전달합니다. 살아 있는 소켓이나 알 수 없는 PID를 지워 교체하지 않습니다.
4. 확인한 새 앱 빌드로 다시 접속합니다. bootstrap이 검증된 bundle과 해당 플랫폼 파일을 준비하고 새 daemon을 시작합니다. `ready`의 build hash·기능 목록과 실제 입력·재접속을 확인합니다.
5. 되돌릴 때도 먼저 소유 작업을 정리하고 같은 명시 중지 절차를 거친 뒤 이전 앱 빌드로 접속합니다. journal을 보존하고 이전 빌드가 같은 저장 형식을 읽을 수 있는지 확인합니다. 저장 형식 호환성이 확인되지 않으면 state 파일을 삭제해 강제로 맞추지 않습니다.

개발 checkout에서는 `pnpm build`가 원격 bundle·PTY manifest까지 생성합니다. 이 빌드가 성공했다고 설치 앱이 바뀌거나 실행 중 daemon이 업그레이드되지는 않습니다. 빌드가 바뀐 같은 namespace의 앱들을 동시에 연결하면 불일치할 수 있으므로 함께 점검합니다.

OS 로그인 시 자동 시작이나 launchd·systemd 등록은 하지 않습니다. 다음 접속의 bootstrap이 서버를 시작합니다. 서버 재시작으로 소유 PTY를 되살리거나 이전 입력을 재실행하는 복구는 제공하지 않습니다.

## 구현 근거

- [호스트 연결·발견](../src/lib/remoteHostConnection.ts)
- [서버 수명·hook 전달·제어](../src/lib/remoteHostServer.ts)
- [원격 PTY 소유·입출력](../src/lib/remoteHostPty.ts)
- [journal 보관·한도](../src/lib/remoteHostHookJournal.ts)
- [hook 재전달의 상태 검증](../src/desktop/main/services/hookService.ts)
- [플랫폼별 원격 PTY 빌드](../scripts/build-remote-host-pty.cjs)
- [호스트별 diff 갱신](../src/desktop/main/services/diffService.ts)
