# KanVibe Mobile

KanVibe 데스크탑에 연결해 보드를 보고, 태스크의 터미널 pane을 그대로 비추는 Flutter 클라이언트.

## 화면

| | 폰 | 태블릿 |
| --- | --- | --- |
| 첫 화면 | 상태별 세로 구획 목록 | 상태별 칸반 |
| 태스크 상세 | 탭 하나 = pane 하나, 고른 pane이 전체 화면 | 탭 하나 = pane 하나, 고른 pane이 전체 화면 |

보드는 너비 768dp에서 배치가 바뀐다(`AppSizes.tabletBreakpoint`). 터미널은 모든 크기에서 pane 하나씩 표시한다. tmux와 일반 terminal은 14sp 글자 크기를 유지하면서 모바일의 가용 영역에 맞춰 원본 터미널의 행·열을 조정한다. 화면을 회전하거나 키보드를 열면 새 영역을 반영하며, 이전 출력은 위아래 스크롤로 읽는다. 정확한 행·열 지정을 지원하지 않는 Zellij와 이전 데스크탑 버전은 원본 격자를 유지하고 좌우·위아래 스크롤을 제공한다.

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
