# 모바일 채팅 입력 구현·검증

현재 소스: `78cad15a5dbaf99460f6f674517712cc22edb89e`. 브랜치 유지, 모바일 외 제품 소스 수정 없음.

- 사진/문서 선택, 이미지 미리보기·제거, 실제 업로드 실패 후 재시도, 설명 포함 전송: `final-picker-file` 녹화 PASS.
- 실제 OS 이미지 클립보드 10개 업로드 → 화면 종료 시 10개 정리 → 같은 CLI 재진입·새 업로드: `final-lifetime-overlay-settled` 녹화 PASS. 서버 SHA 검사로 10→0→1→0 검증.
- 이미지 단독 붙여넣기 전송: `final-paste-drop-probe` 녹화 PASS. 이름과 달리 이 흐름의 drop 부분은 입력 도구 준비까지만이며 실제 drop은 아래 독립 흐름에서 검증했다.
- 스킬 버튼·직접 `$` 입력은 `final-completions`의 두 CLI ACK 단계 PASS. 이 전체 흐름은 IME Enter가 줄바꿈을 넣어 실패했으며 실패 증거를 보존했다. 포커스를 유지한 실제 Android KeyEvent Enter의 `/prompts:qa-custom` 선택과 공백 있는 `@` 파일 선택·전송은 `final-completion-native-key` 녹화 PASS. UI 문구/실제 필드값으로 IME 동작과 KeyEvent 경로를 구분했다.
- 종료된 세션의 설명 포함 이미지·이미지 단독 행, 없는 이미지의 오류와 재시도: `final-history-row` 녹화 PASS. 실제 빨강/파랑 이미지가 디코딩된 화면을 직접 확인했다.
- 다른 앱의 실제 OS GLOBAL_URI_READ 드래그 → 미리보기·업로드 → 설명 포함 CLI ACK: `final-overlay-drop` 녹화 PASS. 서버 파일 SHA가 테스트 PNG와 정확히 일치한다. 초기 helper Activity의 화면 전체 InputSink가 drop을 막았으며 작은 overlay Service로 QA 환경을 고쳤다. 검증되지 않은 FlutterView 제품 패치는 철회하여 원래 DecorView 구현을 유지했다.

`final-transport-and-drop-bytes.json`은 C3 네 요청과 drop 설명이 A에 각각 1회, B에는 0회 전달되었는지와 실제 drop 바이트 일치를 기록한다. 과금 모델 호출 없이 격리된 실제 PTY의 입력을 검사했으며 실제 Codex/Claude provider 실행은 상위 native probe의 별도 증거다.

최종 Flutter `analyze`, 전체 `test` 534/534, profile APK, 수정 Dart 18개 format 모두 exit0. 명령·HEAD·fingerprint는 `final-gates.json`. 45c+fa 트리는 커밋78과 바이트가 동일하며 상위 `final-evidence-source-reuse.json`이 기존 전체 기기 흐름의 모든 mobile 파일 SHA 일치를 확인한다. 기존 미수정 terminal_shortcuts_test.dart의 전체 포맷 차이는 별도 `tracked-format.log`에 보존했다.

회귀 RED/GREEN: intake stale-error 2건, skill button 포커스, sheet 복귀 전 owner 비활성화, 오류 메시지·retry 터치 영역, 폐기된 staged ID/늦은 upload ACK/대기 중 send 정리. `intake-red.jsonl`, `composer-skills-green.log`, `sheet-owner-red.log`, `intake-message-red.log`, `retry-target-red.log`, `lifetime-red.log`과 대응 GREEN 로그를 보존했다. 상태·자원 소유권은 `state-boundary-audit.md`.

iOS 전체 빌드·기기 실행은 기존 Xcode26.0.1/Firebase SDK13의26.2 요구 제약으로 미검증이다. 새 Swift bridge는 SDK typecheck exit0. 실제 외부 하드웨어의 수정키 단축키는 기존 ADB 한계로 미검증이며 widget 경로 검증과 구분한다. HEIC/HEIF 원본은 지원하지 않는 이미지 오류로 표시하며 iOS compatible picker 변환은 캐시된 실제 플러그인 소스에서 확인했다.

기록과 화면·실패 증거는 모두 이 디렉터리에 보존했다. 기기/프로세스 정리는 `cleanup.json`을 따른다.
