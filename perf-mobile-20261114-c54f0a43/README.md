# 모바일 성능·채팅 QA 증거

현재 게이트 검증 커밋: `c54f0a436fa9c694c1b7cedee0276c39b81a30cc`

`gates/`는 해당 커밋에 포함된 트리에서 check·test·build·Flutter analyze·Dart format·Flutter test가 모두 exit 0인 로그입니다. 기록된 diff 지문은 `484b98c38c2947d50ea0f0edd621a2a63a66457b`입니다.

`videos/terminal.mp4`와 `videos/history.mp4` 및 모바일 캡처는 dev 통합 전 `a0720586` 위의 미커밋 변경에서 수행한 과거 기기 증거입니다. 통합 후 기기 재실행은 하지 않았습니다. 전체 화면 휠 전달은 실제 Codex 대신 같은 모드를 사용하는 대역으로 확인했습니다.

`videos/desktop-history-failed.mp4`와 관련 보고서는 히스토리 버튼을 찾지 못해 실패한 기존 Electron E2E 실행 증거입니다. 데스크톱 채팅 3개 캡처는 앱 스타일시트로 그린 구성 예시이며 실제 앱의 파일 미리보기 E2E 통과 증거가 아닙니다.

폰 파일 카드·HTML 미리보기, 체감 연결 시간은 기기에서 검증되지 않았습니다. Cloudflare 원격 빌드는 이 로그에 포함되지 않습니다.

`manifest.json`에 원본 경로와 파일별 해시를 기록했습니다. 기능 브랜치의 인덱스·체크아웃을 바꾸지 않고 별도 증거 브랜치로 보관합니다.
