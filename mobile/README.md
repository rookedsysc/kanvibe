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

코드는 5분 뒤 만료되고 한 번 쓰면 사라진다. 연결을 끊으려면 데스크탑 설정의 기기 목록에서 끊는다.

## 검증

```bash
fvm flutter analyze
fvm dart format --output=none --set-exit-if-changed lib test
fvm flutter test
fvm flutter build apk --debug
```

`flutter_secure_storage`가 API 37 이상을 요구해서 `android/app/build.gradle.kts`의 `compileSdk`를
Flutter 기본값보다 한 단계 올려 두었다.

## 데스크탑 화면을 건드리지 않는다

pane을 비출 때 포커스나 크기를 바꾸는 명령은 하나도 쓰지 않는다. tmux는 `capture-pane`과 `pipe-pane`으로,
zellij는 pane id를 지정한 `dump-screen`으로 읽는다. 이유는 `src/lib/paneMirror.ts` 머리말에 적혀 있다.
