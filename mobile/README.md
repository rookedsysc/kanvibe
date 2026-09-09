# KanVibe Mobile

KanVibe 데스크탑에 연결해 보드를 보고, 태스크의 터미널 pane을 그대로 비추는 Flutter 클라이언트.

## 화면

| | 폰 | 태블릿 |
| --- | --- | --- |
| 첫 화면 | 상태별 세로 구획 목록 | 상태별 칸반 |
| 태스크 상세 | 탭 하나 = pane 하나, 고른 pane이 전체 화면 | 탭 하나 = window 하나, 그 window의 pane이 데스크탑과 같은 배치로 함께 |

갈림길은 너비 768dp 하나뿐이고(`AppSizes.tabletBreakpoint`), 두 배치가 같은 데이터를 쓴다.

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

코드는 5분 뒤 만료되고 한 번 쓰면 사라진다.
연결을 끊는 것은 앱의 **연결 끊기**나 데스크탑 설정의 기기 목록 어느 쪽에서든 된다.
앱에서 끊으면 데스크탑의 기기 목록에서도 이 기기가 지워진다. 데스크탑에 닿지 못한 경우에만
앱이 그 사실을 알려 주고, 그때는 데스크탑 설정에서 한 번 더 지워야 한다.

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

## 데스크탑 화면을 건드리지 않는다

pane을 비출 때 포커스나 크기를 바꾸는 명령은 하나도 쓰지 않는다. tmux는 `capture-pane`과 `pipe-pane`으로,
zellij는 pane id를 지정한 `dump-screen`으로 읽는다. 이유는 `src/lib/paneMirror.ts` 머리말에 적혀 있다.
