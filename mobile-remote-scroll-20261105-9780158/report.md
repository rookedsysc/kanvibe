# 모바일 원격 스크롤 구현·검증

로컬 변경 완료. HEAD `a578d57a7d413d472cf54967f750ee7e460a678e` 위 미커밋 diff `978015807c65af6a57305638bf1bbe80dd8a476a` 기준이다. 기존 PR #407의 코드에는 아직 이 후속 수정이 반영되지 않았다. QA 방식은 prompt-separated self-review다.

## 동작

손가락을 쓸어내리면 tmux·Zellij의 해당 pane이 이전 기록으로 이동하고, 쓸어올리면 최신 방향으로 이동한다. 기록 전체를 모바일에 보내는 tmux -S -2000 및 Zellij --full/tail 설계를 철회했다. 보이는 구간만 다시 받고, 일반 terminal의 로컬 스크롤·기존 알림 설정·resize 뒤 마지막 출력 보존은 유지한다. PC의 같은 pane도 같은 스크롤 상태를 보며 포커스·글로벌 마우스 설정은 바꾸지 않는다.

키 입력 text와 스크롤 binary JSON을 구분한다. 음수=이전, 양수=최신, 0 제외 정수 ±100행 이내다. 같은 소켓의 resize/scroll/key 순서와 같은 pane의 scroll/resume 순서를 보존한다. tmux 기록을 읽는 동안 pipe의 새 출력으로 덮어쓰지 않고, 최신 복귀 또는 입력 때 live output을 재개한다.

## 최종 검증

모든 아래 게이트는 시작 HEAD와 binary diff fingerprint를 기록했고 최종 트리와 일치한다. Node1902→1925(+23), Flutter259→263(+4). Node 추가분은 route7·bridge12·builder4이고, Flutter 추가분은 일반/대체 화면의 원격 gesture2·미세 이동 누적1·dispose 취소1이다. 기존 네트워크/모델 검증도 확장했다. 회귀 RED 증거는 red-route.log, red-gesture.log에 있다.

| Command | Exit | Log |
|---|---|---|
| `pnpm check` | 0 | `verified-check.log` |
| `pnpm build` | 0 | `verified-build.log` |
| `pnpm test` | 0 | `verified-tests.log` |
| `/Users/rookedsysc/fvm/versions/3.47.2/bin/flutter analyze` | 0 | `verified-flutter-analyze.log` |
| `/Users/rookedsysc/fvm/versions/3.47.2/bin/flutter test --machine --concurrency=2` | 0 | `verified-flutter-tests.log` |
| `/Users/rookedsysc/fvm/versions/3.47.2/bin/flutter build apk --debug` | 0 | `verified-android-build.log` |
| `/Users/rookedsysc/fvm/versions/3.47.2/bin/dart format --output=none --set-exit-if-changed mobile/lib/src/common/network/desktop_client.dart mobile/lib/src/features/task_detail/domain/mirror_pane.dart mobile/lib/src/features/task_detail/presentation/task_detail_screen.dart mobile/lib/src/features/task_detail/presentation/widgets/pane_terminal_view.dart mobile/test/support/fake_desktop.dart mobile/test/src/common/network/desktop_client_test.dart mobile/test/src/features/task_detail/mirror_pane_test.dart mobile/test/src/features/task_detail/pane_terminal_view_test.dart` | 0 | `verified-format.log` |
| `git diff --check` | 0 | `verified-diff.log` |
| `python3 qa-results/roky-harness/mobile-remote-scroll/real_tmux.py` | 0 | `verified-real-tmux.log` |
| `python3 qa-results/roky-harness/mobile-remote-scroll/probe_zellij.py` | 0 | `verified-real-zellij.log` |
| `maestro --device emulator-5584 test -e APP_ID=com.kanvibe.kanvibe_mobile --format junit --output qa-results/roky-harness/mobile-remote-scroll/device-junit.xml --test-output-dir qa-results/roky-harness/mobile-remote-scroll/device-output mobile/.maestro/flows/terminal-remote-scroll.yaml` | 0 | `verified-device.log` |

Node24.15.0·pnpm10.34.5, Flutter3.47.2/Dart3.13.2, Maestro2.7.0, tmux3.6a, Zellij0.45.1(QA용 바이너리, 사용자0.43.1 설치 유지).

## 실제 Android 결과

Maestro1/1 PASS, 실행18초. 최종 빌드와 설치 APK SHA256이 같다. 실제 tmux 위치가0→26→0으로 바뀌었고, 화면139–180→113–155→139–180을 세 장의 사진에서 직접 확인했다. 일반/대체 화면 gesture에 키 입력이 함께 나가지 않는 것은 widget 테스트로 확인했다. 영상 terminal-remote-scroll.mp4는540×1200·15fps·약6.9초다. 녹화는 페어링된 보드에서 시작하고 navigation·스와이프를 포함한다. 페어링은 별도 준비 단계다.

실제 운영 builder가 생성한 tmux 명령은24줄 viewport를 이전40행으로 이동했다가 최신180번으로 복귀했다. Zellij는 비활성 pane0을 스크롤하고 활성 pane의 포커스가 유지되는 것을 실제 CLI에서 검증했다. 두 pane 레이아웃의 높이가 달라져 첫 하드코딩 행번호 assertion이 실패했으며, 실제 viewport 첫 행의40행 차이와180번 복귀로 검증을 고쳤다. 해당 실패 로그도 보존했다.

## 품질·경계

18파일/63hunk/미매핑0/behavior-preserving0. obsolete-by-plan은 사전 전체 기록 전송 설계와 그 종속 assertion·문서다. 범위 밖 gate-enabling-repair0, tooling-enforced-format0. 기존 레이어/Flutter feature-first·Riverpod 배치를 유지했고, command builders는 paneMirror, pane 조율은 mobileBridgeService, 직렬화는 DesktopClient/PaneStream, gesture와 timer는 mounted PaneTerminalView가 소유한다. canScrollRemotely는 surfaces에서 유도한 불변 presentation input이며 새 전역 provider·저장소·DB 쓰기는 없다. timer는 pause/retry/dispose 때 정리한다. 새 QA 파일은 기존 flows/scripts 디렉터리를 사용하고 fixture의 기존 probe 서버를 확장했다.

## 환경 복구와 미검증

Android17 Fold에서 접근성 조회137과 ADB shell timeout을 관측했다. 전용 기기 메모리 증설 재시작·해당 transport 재연결 후에도 echo timeout이 남아, 이미 설치된 Android36.1 이미지로 QA 디렉터리 안에 일반 폰 AVD를 만들었다. avdmanager가 없어 기존 config 정의를 사용했고 SDK/사용자 AVD 설정을 변경하지 않았다. 다른 작업의 ComboSettings 기기·ADB 서버는 건드리지 않았다. 새 폰에서 설치·권한·페어링·여정이 모두 통과했다.

새 SSH 테스트의 첫 실패는 테스트용 호스트 설정이 없었기 때문이며 FS 경계에 설정 응답을 두어 고쳤다. 제품 코드를 우회하거나 assertion을 줄여 통과시키지 않았다.

iOS는 현재 Xcode26.0.1/Swift6.2와 기존 Firebase Swift tools6.2.1 요구 차이로 미검증이다. Zellij Android native 통합, 실제 SSH 호스트, 이 미커밋 diff에 대한 원격 CI는 미검증이다. Cloudflare Workers Builds는 로컬 재현할 수 없으며 기존 PR의 이전 성공 기록은 새 diff 검증으로 인용하지 않는다.

## 변경 파일

- `electron/mobileRoutes.js`
- `electron/mobileRoutes.test.mjs`
- `mobile/.maestro/flows/terminal-remote-scroll.yaml`
- `mobile/.maestro/scripts/assert-terminal-scroll.js`
- `mobile/README.md`
- `mobile/lib/src/common/network/desktop_client.dart`
- `mobile/lib/src/features/task_detail/domain/mirror_pane.dart`
- `mobile/lib/src/features/task_detail/presentation/task_detail_screen.dart`
- `mobile/lib/src/features/task_detail/presentation/widgets/pane_terminal_view.dart`
- `mobile/test/src/common/network/desktop_client_test.dart`
- `mobile/test/src/features/task_detail/mirror_pane_test.dart`
- `mobile/test/src/features/task_detail/pane_terminal_view_test.dart`
- `mobile/test/support/fake_desktop.dart`
- `qa/mobile/lib/prepareQaDesktop.cjs`
- `src/desktop/main/services/__tests__/mobileBridgeService.test.ts`
- `src/desktop/main/services/mobileBridgeService.ts`
- `src/lib/__tests__/paneMirror.test.ts`
- `src/lib/paneMirror.ts`
