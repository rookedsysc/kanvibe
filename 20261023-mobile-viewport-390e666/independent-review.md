# 모바일 terminal viewport 독립 코드 검토

Review verdict: **PASS (위임된 source/contract 검토 범위)**. 잔여 correctness/security/lifecycle/복원/동시성 blocker: **0**.

Specification conformance: **UNVERIFIED (전체 delivery)**. A1/A2 코드 계약과 142개 service/wire/parser 테스트는 PASS다. T6 실제 tmux stty·zoom·focus 증거와 T7 Galaxy/MP4, Flutter 전체·CI·APK gate는 coordinator가 진행 중이며 이 reviewer는 실행·인증하지 않았다. 이는 코드 수정 요청이 아니라 검증 소유권의 경계다.

Implementation quality: **PASS (검토 범위)**. 적용 기준은 승인 계획의 기존 service→lib / Flutter feature 경계, shared design-quality·comment-quality와 code-organization·SOLID다. 별도 아키텍처 스타일 선언은 없으며 변경된 심볼·인접 주석만 검토했다.

Review execution: **isolated reviewer** (`/root/viewport_qa`). production source 편집, 빌드, 실제 terminal/device 세션 조작, 커밋·push는 수행하지 않았다. 아래 격리 Node VM은 source 본문을 읽어 실행하고 I/O만 fake했다. shell cleanup 검증에서는 함수 `tmux()`를 fake하여 실제 tmux 명령을 실행하지 않았다.

Reviewed HEAD: `4bb5a0192aed6c62d26328213f1e1e3966bbaa4b`
Reviewed tracked diff SHA-256: `ddb3fc06958c49d4c1e61e7e5e6d2756687cf7003c05fbc47d08764df5a5eac0`
Reviewed tracked diff Git object fingerprint: `b6b7ce49a594451170e4bced3b5dea43bd6781ad`

## 확인한 문제와 수리 검증

| ID | 도달 가능한 경로와 초기 결함 | 최종 수리·검증 |
|---|---|---|
| Q1 | 모바일 활성 상태에서 desktop Quit → `stopAllPaneMirrors()`의 void restore가 before-quit 이후로 밀리고 PTY도 먼저 종료 | `electron/main.js:1150` preventDefault/재진입 guard → Promise 복원 완료 → PTY 정리 → quit. 실제 callback 본문을 격리 VM으로 실행해 이 순서와 재진입 차단 확인 |
| Q2 | A/B 동시 퇴장 → 첫 release가 group을 일찍 삭제 → 두 번째 restore가 대기 중 C가 새 group에 resize → 늦은 restore가 C 크기를 덮음 | `mobileBridgeService.ts:680` pending Promise identity가 최신인 경우에만 map 정리. 같은 source 본문의 guard 없는 파생본은 최종 120×30, 최종본은 C의 60×50을 유지. package 회귀 테스트도 추가됨 |
| Q3 | tmux split을 PC에서 zoom한 뒤 모바일 접속 → resize-pane -x/-y가 unzoom, 저장 state가 zoom을 누락해 퇴장 후 PC zoom 해제. zoomed geometry로 계산하면 split 여유 공간도 잘못 계산 | `paneMirror.ts:67` zoom flag 보관, `:80` unzoom 후 geometry, `:99` 정책/배치 뒤 현재 focus의 pane에 zoom 복원. source 계약 확인; 실제 tmux 통합 증거는 coordinator 소유 |
| Q4 | Quit가 map을 수집하는 동안 첫 claim의 DB/list-panes가 대기 → 뒤늦게 새 group 생성·resize → shutdown 복원 Promise 밖에 남음 | `mobileBridgeService.ts:633` 진입 및 마지막 await 뒤 shutdown fence. 실제 source claim/stop 본문으로 대기 중 첫 claim·shutdown 이후 새 claim 모두 거절하고 driver 생성 0개 확인 |
| Q5 | PC가 모바일 접속 중 pane 추가 → 저장 layout보다 pane 수가 많아 select-layout 실패 → `&&` 체인 뒤의 window-size 정책 복원 누락 | `paneMirror.ts:109` 원 오류 상태를 보관하고 정책/zoom cleanup 후 반환. fake tmux의 select-layout을 exit42로 실패시켜도 set-option/if-shell 실행되고 최종42 반환 확인 |

Q3/Q5의 tmux 의미는 로컬 tmux 3.6a man(`window_layout`은 zoom을 무시하며 pane 수가 더 많은 창에 저장 layout 적용 불가) 및 공식 소스로 확인했다. [tmux 3.6a resize-pane](https://github.com/tmux/tmux/blob/3.6a/cmd-resize-pane.c#L79-L87), [select-layout](https://github.com/tmux/tmux/blob/3.6a/cmd-select-layout.c)에서 unzoom 동작을 확인할 수 있다. 위 소스 해석 외 실제 tmux 성공은 이 보고서가 주장하지 않는다.

## 명령·증거

- `git diff --check`: exit 0.
- source 추출 Node VM pending guard harness: exit 0. guard 없음 → `jumpedQueue=true`, C 연결 중 최종120×30; 현재 → `jumpedQueue=false`, 최종60×50.
- source 추출 Node VM shutdown/before-quit 및 fake shell cleanup harness: exit 0. 대기 중/신규 claim 거절; `restore-complete < kill-pty < quit`; layout 실패42에도 정책·zoom cleanup 실행, 원42 반환.
- coordinator artifact `backend-final.json`: exit 0, before/after tree 일치, 위 SHA-256과 일치.
- `backend-report.json`: **142 passed / 0 failed**, 선택 파일은 계획의 mobileBridgeService.test.ts, electron/mobileRoutes.test.mjs, paneMirror.test.ts 세 개와 정확히 일치. 새 backend/wire 테스트 7개(서비스5·wire2), 계획 T3/T4/T5를 포함한다. Node VM 검증은 이 package test 수에 합산하지 않는다.
- `backend-final.json`의 실제 명령: `pnpm exec vitest run src/desktop/main/services/__tests__/mobileBridgeService.test.ts electron/mobileRoutes.test.mjs src/lib/__tests__/paneMirror.test.ts --reporter=json --outputFile=qa-results/roky-harness/20261023-mobile-viewport/backend-report.json`.
- Recording: 계획은 required, canonical Maestro startRecording wrapper. device·result.json·최종 MP4·실패 MP4·sensitive-data 처리는 coordinator의 최종 증거를 기다리며 이 보고서 범위에서 UNVERIFIED다.

## UI state/component audit

| Input/resource | Before → After | Canonical owner / actual consumer / lifetime |
|---|---|---|
| taskId, pane | 선택 snapshot → 유지 | TaskDetailScreen / PaneTerminalView / widget |
| canResize | 없음 → immutable presentation input | TaskSurfaces API snapshot, surfacesProvider → TaskDetailScreen이 선택 pane과 atomic input으로 전달 → PaneTerminalView가 렌더링·resize 허용 판단 |
| terminal, stream, subscription, timer, scroll controllers | widget 소유 → 유지 | PaneTerminalViewState / terminal rendering·wire / mount-dispose |
| lastSentSize, isSuspended, streamGeneration | 없음 → local lifecycle bookkeeping | PaneTerminalViewState / 중복 전송·오래된 callback 차단 / 연결·widget |
| resize 직렬화 | 없음 → PaneStream.resize | DesktopClient/PaneStream / binary wire 직렬화 / socket |

provider/store snapshot relay chain 제거 대상은 없다. canResize는 선택 pane/taskId와 함께 유지하는 presentation input이며, 별도 전역 상태를 만들지 않는다. hydrate는 pane 크기 후 실제 layout 자동 크기, retry/resume은 새 stream과 크기 재전송, background/dispose는 timer·subscription·stream 해제 및 generation 무효화다. pane 선택은 ValueKey(pane.id)로 수명을 교체한다. 최종 class/file 구성은 PaneTerminalView/_PaneTerminalViewState/_PaneStreamFailure와 TaskDetailScreen, TaskSurfaces, DesktopClient/PaneStream으로 기존 파일 배치를 유지한다. widget은 wire JSON을 직접 만들지 않는다. source 기반 audit은 완료했고 targeted/full Flutter 실행 결과는 coordinator가 소유한다.

## Diff-to-plan mapping

`git diff -U0`: **69 hunks / 17 tracked files**, unmapped **0**, behavior-preserving **0**, justified gate-enabling-repair **0**, tooling-enforced-format **0**. 신규 production/test 파일은 없다. 미추적 사용자 `Screenshot_20261023_090405.jpg`, `Screenshot_20261023_090420.jpg` 두 장은 위임 지시대로 제외하고 stage/수정하지 않았다. 따라서 해당 이미지에 `git add -N .`를 실행하지 않았다.

교체·이동한 기존 fixed-grid/즉시종료/직접 PTY resize 구문은 승인된 새 동작으로 치환한 `obsolete-by-plan`이다. 테스트 제거는 없다. Q1–Q5 수리는 A1의 복원·소유권 불변식을 만족시키기 위한 behavior change로 매핑했으며, 별도 behavior-preserving 수리가 아니다.

| File | Hunk count | Plan | 목적 |
|---|---:|---|---|
| `electron/main.js` | 3 | A1 / T4,T5 / QA 종료 수리 | 복원 완료를 기다린 뒤 PTY 종료 및 quit 재진입 |
| `electron/mobileRoutes.js` | 4 | A1 / T3,T5 | text 입력과 binary resize 분리, 연결 큐·lease 해제 |
| `electron/mobileRoutes.test.mjs` | 2 | A3 / T1–T7 | 동작 회귀 검증·fixture·사용 정책 문서 |
| `mobile/README.md` | 3 | A3 / T1–T7 | 동작 회귀 검증·fixture·사용 정책 문서 |
| `mobile/lib/src/common/network/desktop_client.dart` | 4 | A2 / T1,T2,T7 | capability 전달, 실제 viewport 및 앱 lifecycle 연결 |
| `mobile/lib/src/features/task_detail/domain/mirror_pane.dart` | 3 | A2 / T1,T2,T7 | capability 전달, 실제 viewport 및 앱 lifecycle 연결 |
| `mobile/lib/src/features/task_detail/presentation/task_detail_screen.dart` | 2 | A2 / T1,T2,T7 | capability 전달, 실제 viewport 및 앱 lifecycle 연결 |
| `mobile/lib/src/features/task_detail/presentation/widgets/pane_terminal_view.dart` | 20 | A2 / T1,T2,T7 | capability 전달, 실제 viewport 및 앱 lifecycle 연결 |
| `mobile/test/src/common/network/desktop_client_test.dart` | 2 | A3 / T1–T7 | 동작 회귀 검증·fixture·사용 정책 문서 |
| `mobile/test/src/features/task_detail/mirror_pane_test.dart` | 1 | A3 / T1–T7 | 동작 회귀 검증·fixture·사용 정책 문서 |
| `mobile/test/src/features/task_detail/pane_terminal_view_test.dart` | 3 | A3 / T1–T7 | 동작 회귀 검증·fixture·사용 정책 문서 |
| `mobile/test/src/features/task_detail/task_detail_screen_test.dart` | 1 | A3 / T1–T7 | 동작 회귀 검증·fixture·사용 정책 문서 |
| `mobile/test/support/fake_desktop.dart` | 2 | A3 / T1–T7 | 동작 회귀 검증·fixture·사용 정책 문서 |
| `src/desktop/main/services/__tests__/mobileBridgeService.test.ts` | 2 | A3 / T1–T7 | 동작 회귀 검증·fixture·사용 정책 문서 |
| `src/desktop/main/services/mobileBridgeService.ts` | 12 | A1 / T2,T4,T5 / QA 경쟁·shutdown 수리 | capability, window lease·직렬화·복원·종료 fence |
| `src/lib/paneMirror.ts` | 2 | A1 / T4,T6 / QA zoom·layout 수리 | tmux geometry·임시 크기·정책·zoom 보관/복원 명령 |
| `src/lib/terminal.ts` | 3 | A1 / T4,T5 | plain PTY의 모바일 크기 우선과 PC 크기 보관/복원 |

| File:new-line | Hunk summary | Mapped to | Behavior change? |
|---|---|---|---|
| `electron/main.js:1150` (+7) | 복원 완료를 기다린 뒤 PTY 종료 및 quit 재진입 | A1 / T4,T5 / QA 종료 수리 | yes — 승인된 동작/검증/문서 |
| `electron/main.js:1160` (+0) | 복원 완료를 기다린 뒤 PTY 종료 및 quit 재진입 | A1 / T4,T5 / QA 종료 수리 | yes — 승인된 동작/검증/문서 |
| `electron/main.js:1165` (+9) | 복원 완료를 기다린 뒤 PTY 종료 및 quit 재진입 | A1 / T4,T5 / QA 종료 수리 | yes — 승인된 동작/검증/문서 |
| `electron/mobileRoutes.js:372` (+3) | text 입력과 binary resize 분리, 연결 큐·lease 해제 | A1 / T3,T5 | yes — 승인된 동작/검증/문서 |
| `electron/mobileRoutes.js:384` (+3) | text 입력과 binary resize 분리, 연결 큐·lease 해제 | A1 / T3,T5 | yes — 승인된 동작/검증/문서 |
| `electron/mobileRoutes.js:397` (+26) | text 입력과 binary resize 분리, 연결 큐·lease 해제 | A1 / T3,T5 | yes — 승인된 동작/검증/문서 |
| `electron/mobileRoutes.js:446` (+2) | text 입력과 binary resize 분리, 연결 큐·lease 해제 | A1 / T3,T5 | yes — 승인된 동작/검증/문서 |
| `electron/mobileRoutes.test.mjs:42` (+1) | 동작 회귀 검증·fixture·사용 정책 문서 | A3 / T1–T7 | yes — 승인된 동작/검증/문서 |
| `electron/mobileRoutes.test.mjs:453` (+29) | 동작 회귀 검증·fixture·사용 정책 문서 | A3 / T1–T7 | yes — 승인된 동작/검증/문서 |
| `mobile/README.md:12` (+1) | 동작 회귀 검증·fixture·사용 정책 문서 | A3 / T1–T7 | yes — 승인된 동작/검증/문서 |
| `mobile/README.md:92` (+1) | 동작 회귀 검증·fixture·사용 정책 문서 | A3 / T1–T7 | yes — 승인된 동작/검증/문서 |
| `mobile/README.md:94` (+3) | 동작 회귀 검증·fixture·사용 정책 문서 | A3 / T1–T7 | yes — 승인된 동작/검증/문서 |
| `mobile/lib/src/common/network/desktop_client.dart:241` (+3) | capability 전달, 실제 viewport 및 앱 lifecycle 연결 | A2 / T1,T2,T7 | yes — 승인된 동작/검증/문서 |
| `mobile/lib/src/common/network/desktop_client.dart:265` (+1) | capability 전달, 실제 viewport 및 앱 lifecycle 연결 | A2 / T1,T2,T7 | yes — 승인된 동작/검증/문서 |
| `mobile/lib/src/common/network/desktop_client.dart:276` (+2) | capability 전달, 실제 viewport 및 앱 lifecycle 연결 | A2 / T1,T2,T7 | yes — 승인된 동작/검증/문서 |
| `mobile/lib/src/common/network/desktop_client.dart:285` (+2) | capability 전달, 실제 viewport 및 앱 lifecycle 연결 | A2 / T1,T2,T7 | yes — 승인된 동작/검증/문서 |
| `mobile/lib/src/features/task_detail/domain/mirror_pane.dart:65` (+1) | capability 전달, 실제 viewport 및 앱 lifecycle 연결 | A2 / T1,T2,T7 | yes — 승인된 동작/검증/문서 |
| `mobile/lib/src/features/task_detail/domain/mirror_pane.dart:68` (+1) | capability 전달, 실제 viewport 및 앱 lifecycle 연결 | A2 / T1,T2,T7 | yes — 승인된 동작/검증/문서 |
| `mobile/lib/src/features/task_detail/domain/mirror_pane.dart:74` (+1) | capability 전달, 실제 viewport 및 앱 lifecycle 연결 | A2 / T1,T2,T7 | yes — 승인된 동작/검증/문서 |
| `mobile/lib/src/features/task_detail/presentation/task_detail_screen.dart:159` (+3) | capability 전달, 실제 viewport 및 앱 lifecycle 연결 | A2 / T1,T2,T7 | yes — 승인된 동작/검증/문서 |
| `mobile/lib/src/features/task_detail/presentation/task_detail_screen.dart:166` (+7) | capability 전달, 실제 viewport 및 앱 lifecycle 연결 | A2 / T1,T2,T7 | yes — 승인된 동작/검증/문서 |
| `mobile/lib/src/features/task_detail/presentation/widgets/pane_terminal_view.dart:15` (+1) | capability 전달, 실제 viewport 및 앱 lifecycle 연결 | A2 / T1,T2,T7 | yes — 승인된 동작/검증/문서 |
| `mobile/lib/src/features/task_detail/presentation/widgets/pane_terminal_view.dart:17` (+6) | capability 전달, 실제 viewport 및 앱 lifecycle 연결 | A2 / T1,T2,T7 | yes — 승인된 동작/검증/문서 |
| `mobile/lib/src/features/task_detail/presentation/widgets/pane_terminal_view.dart:26` (+1) | capability 전달, 실제 viewport 및 앱 lifecycle 연결 | A2 / T1,T2,T7 | yes — 승인된 동작/검증/문서 |
| `mobile/lib/src/features/task_detail/presentation/widgets/pane_terminal_view.dart:32` (+2) | capability 전달, 실제 viewport 및 앱 lifecycle 연결 | A2 / T1,T2,T7 | yes — 승인된 동작/검증/문서 |
| `mobile/lib/src/features/task_detail/presentation/widgets/pane_terminal_view.dart:43` (+3) | capability 전달, 실제 viewport 및 앱 lifecycle 연결 | A2 / T1,T2,T7 | yes — 승인된 동작/검증/문서 |
| `mobile/lib/src/features/task_detail/presentation/widgets/pane_terminal_view.dart:60` (+1) | capability 전달, 실제 viewport 및 앱 lifecycle 연결 | A2 / T1,T2,T7 | yes — 승인된 동작/검증/문서 |
| `mobile/lib/src/features/task_detail/presentation/widgets/pane_terminal_view.dart:67` (+1) | capability 전달, 실제 viewport 및 앱 lifecycle 연결 | A2 / T1,T2,T7 | yes — 승인된 동작/검증/문서 |
| `mobile/lib/src/features/task_detail/presentation/widgets/pane_terminal_view.dart:76` (+1) | capability 전달, 실제 viewport 및 앱 lifecycle 연결 | A2 / T1,T2,T7 | yes — 승인된 동작/검증/문서 |
| `mobile/lib/src/features/task_detail/presentation/widgets/pane_terminal_view.dart:78` (+4) | capability 전달, 실제 viewport 및 앱 lifecycle 연결 | A2 / T1,T2,T7 | yes — 승인된 동작/검증/문서 |
| `mobile/lib/src/features/task_detail/presentation/widgets/pane_terminal_view.dart:89` (+30) | capability 전달, 실제 viewport 및 앱 lifecycle 연결 | A2 / T1,T2,T7 | yes — 승인된 동작/검증/문서 |
| `mobile/lib/src/features/task_detail/presentation/widgets/pane_terminal_view.dart:120` (+1) | capability 전달, 실제 viewport 및 앱 lifecycle 연결 | A2 / T1,T2,T7 | yes — 승인된 동작/검증/문서 |
| `mobile/lib/src/features/task_detail/presentation/widgets/pane_terminal_view.dart:127` (+1) | capability 전달, 실제 viewport 및 앱 lifecycle 연결 | A2 / T1,T2,T7 | yes — 승인된 동작/검증/문서 |
| `mobile/lib/src/features/task_detail/presentation/widgets/pane_terminal_view.dart:137` (+1) | capability 전달, 실제 viewport 및 앱 lifecycle 연결 | A2 / T1,T2,T7 | yes — 승인된 동작/검증/문서 |
| `mobile/lib/src/features/task_detail/presentation/widgets/pane_terminal_view.dart:139` (+11) | capability 전달, 실제 viewport 및 앱 lifecycle 연결 | A2 / T1,T2,T7 | yes — 승인된 동작/검증/문서 |
| `mobile/lib/src/features/task_detail/presentation/widgets/pane_terminal_view.dart:151` (+1) | capability 전달, 실제 viewport 및 앱 lifecycle 연결 | A2 / T1,T2,T7 | yes — 승인된 동작/검증/문서 |
| `mobile/lib/src/features/task_detail/presentation/widgets/pane_terminal_view.dart:153` (+3) | capability 전달, 실제 viewport 및 앱 lifecycle 연결 | A2 / T1,T2,T7 | yes — 승인된 동작/검증/문서 |
| `mobile/lib/src/features/task_detail/presentation/widgets/pane_terminal_view.dart:213` (+2) | capability 전달, 실제 viewport 및 앱 lifecycle 연결 | A2 / T1,T2,T7 | yes — 승인된 동작/검증/문서 |
| `mobile/lib/src/features/task_detail/presentation/widgets/pane_terminal_view.dart:260` (+1) | capability 전달, 실제 viewport 및 앱 lifecycle 연결 | A2 / T1,T2,T7 | yes — 승인된 동작/검증/문서 |
| `mobile/lib/src/features/task_detail/presentation/widgets/pane_terminal_view.dart:278` (+3) | capability 전달, 실제 viewport 및 앱 lifecycle 연결 | A2 / T1,T2,T7 | yes — 승인된 동작/검증/문서 |
| `mobile/lib/src/features/task_detail/presentation/widgets/pane_terminal_view.dart:297` (+1) | capability 전달, 실제 viewport 및 앱 lifecycle 연결 | A2 / T1,T2,T7 | yes — 승인된 동작/검증/문서 |
| `mobile/test/src/common/network/desktop_client_test.dart:1` (+4) | 동작 회귀 검증·fixture·사용 정책 문서 | A3 / T1–T7 | yes — 승인된 동작/검증/문서 |
| `mobile/test/src/common/network/desktop_client_test.dart:23` (+43) | 동작 회귀 검증·fixture·사용 정책 문서 | A3 / T1–T7 | yes — 승인된 동작/검증/문서 |
| `mobile/test/src/features/task_detail/mirror_pane_test.dart:29` (+5) | 동작 회귀 검증·fixture·사용 정책 문서 | A3 / T1–T7 | yes — 승인된 동작/검증/문서 |
| `mobile/test/src/features/task_detail/pane_terminal_view_test.dart:31` (+1) | 동작 회귀 검증·fixture·사용 정책 문서 | A3 / T1–T7 | yes — 승인된 동작/검증/문서 |
| `mobile/test/src/features/task_detail/pane_terminal_view_test.dart:45` (+1) | 동작 회귀 검증·fixture·사용 정책 문서 | A3 / T1–T7 | yes — 승인된 동작/검증/문서 |
| `mobile/test/src/features/task_detail/pane_terminal_view_test.dart:131` (+109) | 동작 회귀 검증·fixture·사용 정책 문서 | A3 / T1–T7 | yes — 승인된 동작/검증/문서 |
| `mobile/test/src/features/task_detail/task_detail_screen_test.dart:58` (+4) | 동작 회귀 검증·fixture·사용 정책 문서 | A3 / T1–T7 | yes — 승인된 동작/검증/문서 |
| `mobile/test/support/fake_desktop.dart:95` (+1) | 동작 회귀 검증·fixture·사용 정책 문서 | A3 / T1–T7 | yes — 승인된 동작/검증/문서 |
| `mobile/test/support/fake_desktop.dart:179` (+4) | 동작 회귀 검증·fixture·사용 정책 문서 | A3 / T1–T7 | yes — 승인된 동작/검증/문서 |
| `src/desktop/main/services/__tests__/mobileBridgeService.test.ts:94` (+1) | 동작 회귀 검증·fixture·사용 정책 문서 | A3 / T1–T7 | yes — 승인된 동작/검증/문서 |
| `src/desktop/main/services/__tests__/mobileBridgeService.test.ts:118` (+84) | 동작 회귀 검증·fixture·사용 정책 문서 | A3 / T1–T7 | yes — 승인된 동작/검증/문서 |
| `src/desktop/main/services/mobileBridgeService.ts:30` (+4) | capability, window lease·직렬화·복원·종료 fence | A1 / T2,T4,T5 / QA 경쟁·shutdown 수리 | yes — 승인된 동작/검증/문서 |
| `src/desktop/main/services/mobileBridgeService.ts:40` (+1) | capability, window lease·직렬화·복원·종료 fence | A1 / T2,T4,T5 / QA 경쟁·shutdown 수리 | yes — 승인된 동작/검증/문서 |
| `src/desktop/main/services/mobileBridgeService.ts:50` (+2) | capability, window lease·직렬화·복원·종료 fence | A1 / T2,T4,T5 / QA 경쟁·shutdown 수리 | yes — 승인된 동작/검증/문서 |
| `src/desktop/main/services/mobileBridgeService.ts:59` (+2) | capability, window lease·직렬화·복원·종료 fence | A1 / T2,T4,T5 / QA 경쟁·shutdown 수리 | yes — 승인된 동작/검증/문서 |
| `src/desktop/main/services/mobileBridgeService.ts:112` (+1) | capability, window lease·직렬화·복원·종료 fence | A1 / T2,T4,T5 / QA 경쟁·shutdown 수리 | yes — 승인된 동작/검증/문서 |
| `src/desktop/main/services/mobileBridgeService.ts:167` (+1) | capability, window lease·직렬화·복원·종료 fence | A1 / T2,T4,T5 / QA 경쟁·shutdown 수리 | yes — 승인된 동작/검증/문서 |
| `src/desktop/main/services/mobileBridgeService.ts:177` (+1) | capability, window lease·직렬화·복원·종료 fence | A1 / T2,T4,T5 / QA 경쟁·shutdown 수리 | yes — 승인된 동작/검증/문서 |
| `src/desktop/main/services/mobileBridgeService.ts:579` (+109) | capability, window lease·직렬화·복원·종료 fence | A1 / T2,T4,T5 / QA 경쟁·shutdown 수리 | yes — 승인된 동작/검증/문서 |
| `src/desktop/main/services/mobileBridgeService.ts:838` (+1) | capability, window lease·직렬화·복원·종료 fence | A1 / T2,T4,T5 / QA 경쟁·shutdown 수리 | yes — 승인된 동작/검증/문서 |
| `src/desktop/main/services/mobileBridgeService.ts:895` (+1) | capability, window lease·직렬화·복원·종료 fence | A1 / T2,T4,T5 / QA 경쟁·shutdown 수리 | yes — 승인된 동작/검증/문서 |
| `src/desktop/main/services/mobileBridgeService.ts:897` (+11) | capability, window lease·직렬화·복원·종료 fence | A1 / T2,T4,T5 / QA 경쟁·shutdown 수리 | yes — 승인된 동작/검증/문서 |
| `src/desktop/main/services/mobileBridgeService.ts:912` (+1) | capability, window lease·직렬화·복원·종료 fence | A1 / T2,T4,T5 / QA 경쟁·shutdown 수리 | yes — 승인된 동작/검증/문서 |
| `src/lib/paneMirror.ts:10` (+1) | tmux geometry·임시 크기·정책·zoom 보관/복원 명령 | A1 / T4,T6 / QA zoom·layout 수리 | yes — 승인된 동작/검증/문서 |
| `src/lib/paneMirror.ts:59` (+51) | tmux geometry·임시 크기·정책·zoom 보관/복원 명령 | A1 / T4,T6 / QA zoom·layout 수리 | yes — 승인된 동작/검증/문서 |
| `src/lib/terminal.ts:36` (+2) | plain PTY의 모바일 크기 우선과 PC 크기 보관/복원 | A1 / T4,T5 | yes — 승인된 동작/검증/문서 |
| `src/lib/terminal.ts:919` (+2) | plain PTY의 모바일 크기 우선과 PC 크기 보관/복원 | A1 / T4,T5 | yes — 승인된 동작/검증/문서 |
| `src/lib/terminal.ts:1009` (+22) | plain PTY의 모바일 크기 우선과 PC 크기 보관/복원 | A1 / T4,T5 | yes — 승인된 동작/검증/문서 |
