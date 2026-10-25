# KanVibe Mobile

KanVibe 데스크탑에 연결해 보드를 보고, 태스크의 터미널 pane을 그대로 비추는 Flutter 클라이언트.

## 화면

| | 폰 | 태블릿 |
| --- | --- | --- |
| 첫 화면 | 상태별 세로 구획 목록 | 상태별 칸반 |
| 태스크 상세 | 탭 하나 = pane 하나, 고른 pane이 전체 화면 | 탭 하나 = pane 하나, 고른 pane이 전체 화면 |

보드는 너비 768dp에서 배치가 바뀐다(`AppSizes.tabletBreakpoint`). 터미널은 모든 크기에서 pane 하나씩 표시한다. tmux와 일반 terminal은 14sp 글자 크기를 유지하면서 모바일의 가용 영역에 맞춰 원본 터미널의 행·열을 조정한다. 화면을 회전하거나 키보드를 열면 새 영역을 반영하며, 이전 출력은 위아래 스크롤로 읽는다. 정확한 행·열 지정을 지원하지 않는 Zellij와 이전 데스크탑 버전은 원본 격자를 유지하고 좌우·위아래 스크롤을 제공한다.

터미널의 **텍스트 선택·복사** 버튼을 누르면 현재 출력을 일반 글처럼 읽을 수 있다. 글을 길게 눌러 선택 범위를 조절한 뒤 시스템 메뉴에서 복사한다. 선택 화면은 열었을 때의 출력을 유지하며, **터미널로 돌아가기**를 누르면 실시간 터미널로 돌아온다. **붙여넣기** 버튼은 클립보드 내용을 터미널로 보낸다.

프로젝트 색상은 데스크탑 지정 색상을 사용하며, 지정하지 않았으면 데스크탑과 같은 이름 해시 팔레트를 사용한다. 보드 카드의 테두리·프로젝트 표시와 터미널 헤더에 반영한다.

## 준비

Flutter 버전은 `.fvmrc`에 고정되어 있다.

```bash
fvm install          # .fvmrc에 적힌 버전을 받는다
fvm flutter pub get
```

## 실행

```bash
fvm flutter run
```

## 데스크탑에 연결하기

1. 데스크탑 KanVibe에서 **설정 → 모바일 연결 → 연결 코드 만들기**를 누른다.
2. 앱에 데스크탑 주소와 포트, 화면에 뜬 6자리 코드를 입력한다.
   포트는 패키지된 앱이 `9736`, 개발 빌드가 `19736`이다.
3. 한 번 연결하면 기기에 열쇠가 저장되어 다음부터는 바로 보드가 열린다.

다른 PC를 추가하려면 보드 상단의 현재 PC 이름을 누른 뒤 **PC 추가**를 선택한다. PC마다 구분할
이름을 붙일 수 있고, 주소·포트·연결 토큰은 기기의 보안 저장소에 함께 보관된다. 목록에서 PC를
누르면 해당 PC의 보드로 전환된다. 이전 PC의 연결은 유지되므로 다시 코드를 입력할 필요가 없다.
폰과 태블릿에서 같은 방식으로 전환한다. PC가 꺼져 있어도 목록에서 선택할 수 있으며, 보드에서
연결 오류가 나면 상단의 PC 이름으로 돌아가 다른 PC를 고르면 된다.

코드는 5분 뒤 만료되고 한 번 쓰면 사라진다.
PC 연결을 끊으려면 **연결된 PC** 목록에서 해당 PC의 삭제 버튼을 누른다. 앱에서 삭제하면 그
PC의 기기 목록에서도 이 기기가 지워지며, 다른 PC의 연결은 유지된다. PC에 닿지 못한 경우에는
앱이 그 사실을 알려 주고, 해당 PC의 설정에서 이 기기를 한 번 더 지워야 한다. 현재 PC를 삭제해도
다른 PC로 자동 전환하지 않으므로 목록에서 다음 PC를 고른다.

## Hook 상태 알림

앱을 사용하는 동안 저장된 모든 PC의 `/api/mobile/events`에 인증된 WebSocket으로 연결한다. hook 상태 변경을 받으면 보드와 열려 있는 태스크의 상태를 갱신하고 시스템 알림을 표시한다. 다른 PC의 알림도 받을 수 있으며, WebSocket과 전경 푸시로 같은 이벤트가 오면 한 번만 표시한다.

알림을 누르면 알림을 보낸 PC를 선택하고 `/api/mobile/tasks/:id`에서 최신 태스크를 조회한 뒤 상세 화면을 연다. 보드의 첫 페이지에 없는 태스크와 앱이 종료된 상태에서 누른 알림도 같은 방식으로 처리한다. 태스크가 삭제되었거나 PC 연결이 해제되었으면 안내를 표시한다.

백그라운드와 화면 잠금에서는 이벤트·터미널 소켓을 닫고 FCM/APNs의 시스템 알림을 사용한다. 앱으로 돌아오면 다시 연결해 보드를 조회한다. Android의 설정에서 강제 종료한 앱은 다시 열기 전까지 푸시를 받지 못하며, iOS의 백그라운드 코드 실행도 보장되지 않는다. 백그라운드에서 앱 코드가 상시 동작하는 것을 전제로 하지 않는다. [FCM 수신 공식 문서](https://firebase.google.com/docs/cloud-messaging/flutter/receive-messages)

### 백그라운드 알림 설정

Firebase 설정 없이도 앱과 전경 WebSocket 알림은 동작한다. **백그라운드 수신에는 모바일 Firebase 설정과 데스크탑의 푸시 중계 연결이 모두 필요하다.**

1. 같은 Firebase 프로젝트에 Android 앱 `com.kanvibe.kanvibe_mobile`과 iOS 앱 `com.kanvibe.kanvibeMobile`을 등록한다. iOS는 Firebase에 APNs 인증 키를 등록하고, Apple Developer의 App ID와 서명 프로파일에도 Push Notifications를 활성화한다. Xcode의 Debug는 development, Profile/Release는 production APNs 환경을 사용한다. [FCM Flutter 설정 공식 문서](https://firebase.google.com/docs/cloud-messaging/flutter/get-started)
2. [Android 설정 예시](config/firebase.android.example.json) 또는 [iOS 설정 예시](config/firebase.ios.example.json)를 `config/firebase.android.local.json` 또는 `config/firebase.ios.local.json`으로 복사하고 해당 앱의 실제 Firebase 공개 설정값을 넣는다. Android와 iOS의 앱 ID·API 키는 각 앱에 맞는 값을 사용한다. 이 파일에 서비스 계정 비밀 키나 중계 인증 비밀값을 넣지 않는다.
3. 모바일 디렉터리에서 플랫폼에 맞게 빌드하고 OS의 알림 권한을 허용한다.

```bash
fvm flutter run --dart-define-from-file=config/firebase.android.local.json
fvm flutter build apk --release --dart-define-from-file=config/firebase.android.local.json
fvm flutter run --dart-define-from-file=config/firebase.ios.local.json
```

Android는 같은 빌드 설정을 네이티브 Firebase 리소스에도 반영하므로 앱 프로세스가 없는 상태에서도 시스템 알림을 받을 수 있다. iOS는 Firebase Messaging 17의 FlutterAppDelegate 알림 처리 수정이 필요하여 Firebase iOS SDK 13을 사용한다. iOS 최소 버전은 기존과 같은 15.0이며, **빌드에는 Xcode 26.2 이상이 필요하다.** Xcode 26.0.1에서는 Swift 패키지 해석 단계에서 실패한다. [Firebase iOS 요구사항](https://firebase.google.com/docs/ios/setup), [알림 처리 수정](https://github.com/firebase/flutterfire/pull/18701)

4. [푸시 중계 서버 설정](../services/mobile-push-relay/README.md)에 따라 서비스 계정을 서버에만 설정한다. 데스크탑 KanVibe를 실행하는 프로세스에 `KANVIBE_MOBILE_PUSH_RELAY_URL`과 `KANVIBE_MOBILE_PUSH_RELAY_TOKEN`을 제공한다. URL은 `/notifications`까지 포함하며 비밀값은 중계 서버의 `relayToken`과 같아야 한다. `.env.example`은 예시이고 데스크탑이 자동으로 읽지 않는다.

```bash
KANVIBE_MOBILE_PUSH_RELAY_URL=http://127.0.0.1:4890/notifications \
KANVIBE_MOBILE_PUSH_RELAY_TOKEN='<relay-config-secret>' pnpm dev
```

위 명령은 저장소 루트에서 실행한다. 외부 중계 서버는 HTTPS 주소를 사용한다. 푸시 토큰은 모바일이 페어링된 PC마다 인증 후 등록하고 갱신하며, PC 연결을 해제하면 해당 PC의 수신 정보도 함께 제거한다. 중계 요청은 페어링 인증 토큰을 포함하지 않는다.

시스템 푸시 수신은 인터넷을 통해 이뤄지지만, 알림을 누른 뒤의 태스크 조회와 터미널 연결은 저장한 PC 주소로 직접 접속한다. 외부 네트워크에서 사용하려면 Tailscale 등으로 해당 주소에 도달할 수 있어야 한다. PC의 KanVibe가 실행 중이어야 새 hook 이벤트를 발송할 수 있다.

### 수신 구조와 확인 방법

- `features/notifications/data/mobile_notifications.dart`: Firebase·네이티브 알림 표시와 초기/전경 클릭을 처리한다.
- `features/notifications/application/mobile_notification_controller.dart`: PC별 이벤트 구독·재연결·푸시 토큰 갱신·중복 수신을 처리한다.
- `features/notifications/application/notification_navigation_controller.dart`: 저장된 연결에서만 클릭 목적지를 찾고 최신 태스크를 조회한다.
- `PUT/DELETE /api/mobile/notifications`: 현재 인증된 기기의 푸시 수신 정보를 등록·제거한다.

실기기에서는 앱 실행 중, 다른 앱 사용 중, 화면 잠금, 앱 종료 후의 네 경우에 PC hook으로 상태를 변경하고 알림 수신·클릭 이동을 확인한다. 다른 PC를 선택해 둔 상태에서도 같은 확인을 한다. 앱을 다시 열었을 때 보드가 최신 상태인지, 삭제된 태스크와 해제된 PC의 알림이 안내로 처리되는지도 확인한다. Firebase/APNs와 실기기가 없는 자동 테스트는 실제 푸시 전달을 검증하지 않는다.

## 검증

```bash
fvm flutter analyze
fvm dart format --output=none --set-exit-if-changed lib test
fvm flutter test
fvm flutter build apk --debug
fvm flutter build apk --release
```

릴리스 빌드까지 확인하는 이유는, 권한과 평문 트래픽 설정이 debug 매니페스트에만 있어도 `--debug`는
그대로 통과하기 때문이다. 릴리스에서만 요청이 막히는 결함은 여기서만 드러난다.

`flutter_secure_storage`가 API 37 이상을 요구해서 `android/app/build.gradle.kts`의 `compileSdk`를
Flutter 기본값보다 한 단계 올려 두었다.

## 배포용 서명 (Android)

`android/key.properties`가 있으면 릴리스 빌드가 그 키로 서명되고, 없으면 debug 키로 떨어진다.
Play는 debug 키로 서명된 AAB를 받지 않으므로 배포 전에 업로드 키를 한 번 만들어 둔다.

```bash
keytool -genkeypair -v -keystore ~/kanvibe-upload.jks -storetype PKCS12 \
  -keyalg RSA -keysize 2048 -validity 10000 -alias upload
```

```properties
# android/key.properties — 저장소에 올라가지 않는다(android/.gitignore)
storePassword=<위에서 정한 비밀번호>
keyPassword=<위에서 정한 비밀번호>
keyAlias=upload
storeFile=/home/<사용자>/kanvibe-upload.jks
```

`storeFile`의 상대 경로는 `android/`를 기준으로 읽는다. 항목이 하나라도 빠지면 gradle이 그 자리에서
멈춘다 — 조용히 debug 키로 떨어지면 Play가 거절할 때까지 서명이 잘못된 줄 모르기 때문이다.
확인은 `cd android && ./gradlew :app:signingReport`의 `Variant: release` 항목이 `Config: release`인지로 한다.

**이 keystore를 잃으면 같은 앱을 다시 올릴 수 없다.** 비밀번호와 함께 따로 보관한다.

Play에 올릴 때마다 `pubspec.yaml`의 `version:` 뒤 빌드 번호(`0.1.0+1`의 `+1`)를 올려야 한다.
같은 versionCode는 두 번 받지 않는다. iOS는 `flutter build ipa`가 export 단계에서
App Store Connect를 조회해 `CFBundleVersion`을 스스로 올리므로 손댈 것이 없다.

## 모바일 접속 중 터미널 크기

모바일에서 터미널을 보고 있는 동안 PC의 같은 터미널 크기도 바뀐다. 탭을 벗어나거나 앱을 배경으로 보내거나 연결을 닫으면 PC의 크기로 돌아간다. 여러 모바일이 같은 tmux 창이나 terminal을 보고 있다면 마지막으로 크기를 보낸 기기를 우선하고, 그 기기가 나가면 남은 기기의 크기를 적용한다.

tmux는 창 크기 정책과 분할 배치를 함께 보관했다가 마지막 모바일 연결이 끝날 때 복원한다. 포커스는 바꾸지 않는다. 출력은 tmux의 `capture-pane`/`pipe-pane`, Zellij의 pane id를 지정한 `dump-screen`으로 읽는다. wire의 text 프레임은 키 입력, binary JSON의 `type: resize`는 크기 조정으로 구분한다. 모바일 화면 맞춤에는 이 기능을 지원하는 데스크탑 빌드도 필요하다.

터미널의 Powerline/Nerd Font 기호는 [Nerd Fonts Symbols Only v3.5.1](https://github.com/ryanoasis/nerd-fonts/tree/v3.5.1/patched-fonts/NerdFontsSymbolsOnly)을 보조 글꼴로 사용한다. 원본 파일과 MIT 라이선스를 `assets/fonts/`에 포함한다.
