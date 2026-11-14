# 백그라운드 sync QA

검증 커밋: 85f31710a135537c3956141a9642ffb75b1f54ce
검증된 변경 지문: f8ba30a0ef05a67b6c9cd4af3727b274d0d21418

| 검증 | 결과 |
| --- | --- |
| pnpm check | exit 0 |
| pnpm test | exit 0, 3476 tests passed |
| pnpm build | exit 0 |
| flutter test --no-pub --concurrency=2 --reporter json | exit 0, 797 tests passed |
| flutter analyze --no-pub | exit 0, No issues |
| 변경 Dart 18개 format 검사 | exit 0 |
| flutter build apk --debug --no-pub | exit 0 |
| Electron 실제 앱 | 6 checks PASS |
| Android Maestro | exit 0, 1 flow, 0 failures, 정확히 2 cycles |

진행 단계, 실행 중 버튼 잠금, 다른 클라이언트의 중복 시작, 완료 후 재실행, 탭 전환, 알림 내부 검토, 패널 재열기 후 부분 삭제와 실패만 재시도를 검증했습니다. 390px 데스크톱 패널은 left41/right351로 화면 안에 들어옵니다.

외부 Git/PR 제공자만 fixture로 제어하며 실제 메인 서비스·IPC·HTTP·이벤트 소켓·알림 저장소·UI를 통과합니다. Android 영상은 Maestro 내부 녹화(약37초)입니다.

iOS 실행은 설치된 Xcode26.0.1과 기존 Firebase iOS SDK13.0.0의 Swift tools6.2.1 요구가 맞지 않아 미검증입니다. Android API36.1 arm64 전용 에뮬레이터로 실제 모바일 검증을 완료했습니다. 저장소 외부 Workers Builds는 로컬에서 실행하지 않았습니다.

기능 브랜치에는 소스와 재사용 테스트만 포함합니다. 이 브랜치는 리뷰용 QA 증거를 별도로 보관합니다.
