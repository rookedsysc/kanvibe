# 모바일 PC별 필터 QA

- Android 16 / API 36, Maestro 2.7.0: 1 flow, failures 0, MP4 recorded.
- PC별 프로젝트 선택 분리, 앱 재실행 후 복원, 전체 해제 후 재실행 유지 통과.
- 최신 dev 반영 후 pnpm check/test/build exit 0. CI 테스트 2793개 통과.
- Flutter 전체 테스트 440개 및 정적 분석/변경 Dart 포맷 통과.
- iOS는 Xcode 버전 제약으로 미검증. 전체 Dart format은 기존 파일 2개 오류 유지.

기기 영상은 dev 동기화 전 실행한 원본이며, 동기화 전후 mobile/ 및 QA fixture/flow의 내용이 동일함을 확인했다. 최신 dev 반영 후 자동 게이트를 다시 실행했다.

검증 대상 소스: f2fd86c879da4d79d53b5c89657884146c92d203
