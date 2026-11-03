# 모바일 터미널·알림 설정 QA

최종 커밋: `a578d57a7d413d472cf54967f750ee7e460a678e`. PR base: `dev` (`b2bb42a47cc420506401d1cfa2905a14a3dda776`).

## 최종 커밋 검사

- `pnpm check`: exit 0
- `pnpm test --maxWorkers=1`: exit 0, 1902/1902
- `pnpm build`: exit 0
- Flutter 3.47.2 `flutter analyze`: exit 0
- `flutter test --concurrency=2 --machine`: exit 0, 259/259
- Dart format 검사(변경한 13개 Dart 파일): exit 0
- `git diff --check`: exit 0

검사 시작 HEAD와 binary diff의 Git hash를 기록해 push한 최종 커밋과 같은 diff임을 확인했다. fingerprint: `c9239a90f356b6289c0594fbf75ed72db431f9bc`.

## Android 영상과 사진

첨부 native 증거는 dev 통합 전 HEAD `cfe3b319a398902acc899bbc73e22b5d050529f1`와 당시 미커밋 기능 변경에서 수행했다. 통합 이후 native 흐름을 재실행하지 않았다. 통합된 dev 12커밋은 이 기능의 변경 파일에 직접 겹치지 않았고 최종 전체 검사는 위와 같이 재실행했다.

Android 17 Maestro 1/1 exit 0. 설정 Todo 선택, 전체 OFF/ON, 재진입 후 서버 저장값을 확인했다. tmux 터미널의 마지막 180번 출력에서 이전 출력으로 스와이프하고 다시 마지막 180번 출력으로 돌아옴을 화면에서 확인했다. 최종 APK와 설치 APK의 SHA256이 일치했다. 영상은 약 28초다. 페어링은 격리 fixture 준비 단계에서 수행했으며 녹화는 페어링된 보드에서 시작한다.

실제 tmux와 Zellij CLI에서 접속 이전 기록 포함을 검증했고, Zellij는 최대 2,000줄과 dump 실패 종료 코드 보존을 확인했다. Zellij 기기 통합은 미검증이다.

## 미검증 범위

iOS는 설치된 Xcode 26.0.1/Swift 6.2와 Firebase 13의 Swift tools 6.2.1 요구 차이로 빌드가 중단됐다. 실제 FCM/APNs 전송, VoiceOver/TalkBack, Zellij의 모바일 native 통합은 검증하지 않았다.

QA 방식은 단일 에이전트의 prompt-separated self-review다. 별도 검증자의 독립 리뷰라고 주장하지 않는다.
