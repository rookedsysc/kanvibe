Planner mode: full
QA: independent

기능 명세: PASS. 구현 품질: PASS. 전체 하네스: UNVERIFIED.

원격 Vim 등이 OSC 52로 보낸 텍스트를 iOS·Android 시스템 클립보드에 기록하는 경로를 구현했다. 휴대폰 클립보드 읽기에는 응답하지 않는다. 원격 Vim 설정은 mobile/README.md에 안내했다. tmux/일반 terminal의 실시간 스트림 대상이며, Zellij 화면 덤프에서는 기존 선택 복사를 사용한다.

## 검증 대상
- HEAD: `f5f5598c98b927e4d055dcc6caf8f968883896c2`
- 전체 추적 diff SHA256: `6d8ff4c7c12b6e19ad6f71edee888d5a1a411cb5e9592780db37dddfa7027501`
- 제품 코드·테스트·문서의 미커밋 변경 기준이다. 원격 PR 커밋의 검증으로 주장하지 않는다.
- 새 사용자 제품 기능을 자체 추가하지 않았다. 변경은 4파일/8hunk, 미매핑0, 무관한 behavior-preserving0, gate-enabling-repair0, tooling-enforced-format0.
- 원래 미추적 Screenshot_20261023_110540.jpg는 보존했다.

## 기계적 검증
| 정확한 명령 | 종료 | 증거 |
|---|---:|---|
| `fvm dart format --output=none --set-exit-if-changed lib/src/features/task_detail/presentation/widgets/pane_terminal_view.dart test/src/features/task_detail/pane_terminal_view_test.dart` | `0` | `gates/flutter-format-release/result.json` |
| `fvm flutter analyze` | `0` | `gates/flutter-analyze-release/result.json` |
| `fvm flutter test --reporter json` | `0` | `gates/flutter-tests-release/result.json` |
| `fvm flutter build apk --debug` | `0` | `gates/android-debug-release-tree/result.json` |
| `fvm flutter build apk --release` | `0` | `gates/android-release-release-tree/result.json` |
| `maestro --device emulator-5556 test -e APP_ID=com.kanvibe.kanvibe_mobile --test-output-dir qa-results/roky-harness/20261027-mobile-osc52/device-04/test-output --format junit --output qa-results/roky-harness/20261027-mobile-osc52/device-04/junit.xml qa-results/roky-harness/20261027-mobile-osc52/device-04/recording-wrapper.yaml` | `0` | `gates/android-maestro-04/result.json` |
| `npx --yes --package=pnpm@10.34.5 pnpm --dir /Users/rookedsysc/Documents/github/crookedbot/rookedsysc/kanvibe__worktrees/feat-kanvibe-mobile check` | `0` | `gates/ci-host-check-final/result.json` |

위 명령은 모두 최종 HEAD/diff 지문에서 시작·종료가 같았다. Node host check는 Node24.15.0/pnpm10.34.5의 보충 검증이며 Linux CI를 대체하지 않는다.

- Flutter 전체 JSON: 179/179 PASS, 신규 OSC52 7/7 포함. loading pseudo-tests는 제외한다.
- 최종 유효한 초과 payload(1,000,004개의 A)로 갱신한 테스트를 pre-feature HEAD 위젯에서 다시 실행: exit1, 기존36 PASS/신규7 RED. `gates/terminal-valid-oversize-red/`에서 확인한다. 저장한 후보 위젯을 정확한 바이트로 복원한 뒤 최종 전체 테스트를 통과했다.
- Android emulator-5556 실제 여정: native OS Copy로 BEFOREYANK 저장 → 원격 Vim ggyy → upstream vim-oscyank → native OS Paste로 VIM_OSC52_NATIVE_OK 표시, JUnit1/1. 내부 Maestro clipboard shim을 쓰지 않았다.
- `device-04/result.json`, `device-04/junit.xml`, `device-04/environment.json`, `device-04/test-output/`에 MP4·스크린샷·수신 증거를 보존했다. 영상은 현재 세션 소유 영상만 540px/15fps로 압축했다.
- 원격 Vim register 파일은 `device-01/fixture/.../vim-yank-observed.txt`; 실제 bytes `VIM_OSC52_NATIVE_OK\x00`를 확인했다. 플러그인은 격리 fixture에만 설치했고 사용자 Vim/tmux 설정을 수정하지 않았다.
- 실패 device-02는 선택이 축소되어 Copy가 없었던 QA 준비 단계 문제였다. hierarchy의 Paste/Select all로 원인을 구분하고 Select all 단계를 추가했다. 실패 영상/JUnit/hierarchy와 이후 성공 device-03/04를 각각 보존했다.

## 검증 제한
| 게이트 | 결과·근거 |
|---|---|
| `fvm dart format --output=none --set-exit-if-changed lib test` | exit1. 변경하지 않은 create_task_screen.dart 한 곳이 이미 비정형. HEAD 원본과 현재 파일 bytes 동일, 원본 stdin formatter도 exit1(`baseline-format.json`). 변경 파일 format은 exit0. |
| CI `pnpm install --frozen-lockfile` | Linux Docker 컨테이너 API 무응답으로 미실행. |
| CI `pnpm check` | Linux 환경 미실행. macOS 동일 Node24/pnpm10.34.5 host check는 별도 exit0. |
| CI `pnpm test` | Linux Docker 환경 미실행. |
| CI `pnpm build` | Linux Docker 환경 미실행. |
| iOS build/device | 기존 Xcode26.0.1이 저장소 Firebase SDK13 요구26.2 미달. 공통 Flutter 검증만 수행. |
| Workers Builds: kanvibe | 저장소 외부 CI 앱이며 현재 로컬 미커밋 tree에 대해 실행 불가. |

Docker 복구 시도: 기본 proxy timeout → raw socket info28.5.2 확인 → 자체 Linux container 요청, Python runner 호환 API 수정 → list/inspect/run 모두 무진행 → 이 세션의 Docker client만 종료. 공유 Docker daemon은 재시작하지 않았다. 자체 container/volume 제거도 timeout이어서 제거 성공은 미확인(`docker-cleanup-attempt.json`). 해당 실패와 종료 기록을 숨기지 않고 남겼다. 이 제한 때문에 전체 하네스를 PASS로 표시하지 않는다.

## 계획 항목·가정 결과
| 항목 | 결과·근거 |
|---|---|
| OSC52 복사라는 요청 해석 | 실제 원격 Vim yy/native OS paste로 확인. |
| 빈값·상한·UTF8 replacement 정책 | desktop terminalClipboard.ts와 일치, 7개 widget case로 확인. |
| decode/OS 거부 후 스트림 유지 | PlatformException case에서 다음 복사·출력·입력 PASS. |
| 각 완결 요청 한 번/마지막 값 | BEL/ST 및 모든 split boundary에서 호출 개수·내용 PASS. |
| 기존 OS 복사/붙여넣기 경로 공존 | 기존 terminal36건과 전체179건 PASS, 원격 read API 호출0. |
| constructor/서버 계약 유지 | diff/API 변경0, provider·영구 상태·의존성 추가0. |
| Android/iOS 지원 범위 | Android 실제 여정 PASS, iOS native는 미검증 상태로 남김. |

## diff→plan 매핑
| 파일:추가줄 | 설명 | 계획 |
|---|---|---|
| mobile/.maestro/flows/terminal-clipboard.yaml:1–44 | OS sentinel/실제 Vim/native paste 여정 | T4 |
| mobile/README.md:16–30 | 복사 기능·원격 설정·지원 제한 | A3 |
| pane_terminal_view.dart:2 | UTF8/base64 import | A1 |
| pane_terminal_view.dart:58–59 | 기존 desktop 상한 | A2 |
| pane_terminal_view.dart:84–86 | xterm callback 등록 | A1 |
| pane_terminal_view.dart:191–208 | 단방향 decode/OS copy/error isolation | A1/A2 |
| pane_terminal_view_test.dart:1–2 | fixture encoding | T1–T3 |
| pane_terminal_view_test.dart:150–331 | 7개 통합 widget case | T1–T3 |

8hunks/4files/0unmapped. 독립 검토 상세는 evidence/independent-qa/source-review.md. 계획은 .claude/plan/mobile/2610/27-terminal-osc52-clipboard.plan.md. 루프 상태는 .qa-loop/mobile-osc52.json.
