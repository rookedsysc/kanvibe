# 채팅 skill·사진 첨부 QA

판정: REQUEST_CHANGES. 사진 첨부와 skill/명령/파일 자동완성 UI가 구현되어 있지 않다. 모델 선택과 텍스트 전달만 통과한 상태를 첨부 기능 성공으로 보지 않는다.

대상: `feat/chat-ui`, `d99f6d375eb85ed6eeac8d4b15f8092e1bc27fa5`. 제품 소스 변경 0. `source-binding.json`의 HEAD·지문·파일 SHA에 연결된 검증 전용 작업.

| 항목 | 결과 | 근거 |
| --- | --- | --- |
| 데스크톱 사진/파일 선택 버튼 | 미구현 | 실제 renderer 파일 input·첨부 버튼 0 |
| 이미지 붙여넣기·드롭 | 미구현 | PNG File을 담은 DOM 이벤트에도 미리보기·첨부 정보·PTY 전달 없음 |
| 사진+설명 전송 | 텍스트만 전달 | raw PTY delta는 bracketed caption+CR과 정확히 일치 |
| skill 선택 / `$`·`/` 자동완성 | 미구현 | 실제 화면 메뉴 0, 입력 중 PTY 변화 없음 |
| `@` 파일 검색 | 미구현 | 실제 화면 메뉴 0, 입력 중 PTY 변화 없음 |
| 수동 skill 문자열 전송 | 전송 계층 PASS | `$qa-composer-probe`, `/qa-composer-probe` 바이트 대조 |
| 실제 Codex UI에서 skill 한 번 전송 | FAIL | 보내기 후 앱 초안은 지워지지만 CLI 입력줄에 문자열이 남고 30초 내 로컬 provider 요청 없음 |
| 실제 Codex skill 추가 확인 | 조건부 PASS | `CLI 조작 → 선택 / 확인`을 한 번 더 누르면 skill 본문을 포함한 요청·응답·정확한 JSONL 대화 표시 확인 |
| 실제 Codex 일반 메시지 전송 | PASS | 같은 pane에서 일반 문장은 보내기 한 번으로 provider 요청·응답 도착 |
| 실제 CLI skill 해석 | CLI 단독 PASS | Codex0.167.2·Claude2.1.323 모두 SKILL 본문을 localhost provider 요청에 포함, exit0 |
| 이미지 대화 기록 | 미지원 | 실제 공유 파서에서 이미지 단독 content→빈 문자열, 텍스트+이미지→텍스트만 반환 |

`desktop-ui/result.json`의 8개 시나리오와 버튼 inventory를 확인했다. 이벤트는 실제 앱 renderer에 전달한 `isTrusted:false` ClipboardEvent/DragEvent이다. OS 클립보드는 읽거나 변경하지 않았으며, 사용자 OS 키보드 paste 기본 동작까지 검증했다고 주장하지 않는다. 소스에도 해당 chat textarea의 paste/drop/file handler가 없다.

기존 `Terminal.tsx` 원격 터미널 이미지 붙여넣기는 별도 경로다. 현재 pane chat 입력창이 해당 경로를 사용한다는 근거는 없다. 기존 터미널 기능 전체가 고장났다는 판정은 하지 않는다.

실제 CLI 검증은 격리 HOME·cwd, 테스트 SKILL.md, localhost HTTP 응답으로 수행했다. 유료 모델·사용자 자격증명·프로젝트 hook을 사용하지 않았다. CLI 자체가 skill 본문을 읽은 사실과 Kanvibe UI에서 실제 provider까지 연결됐는지는 별도 판정한다. 생성된 모델 응답은 테스트 응답이며 실제 모델의 skill 수행 결과가 아니다.

모바일은 소유 Android emulator-5590에서 실제 Maestro 1/1, 문자열 전달·다른 pane 격리 6개 assertion을 통과했다. 원본 모바일 218개 입력 파일과 설치 APK SHA가 일치한다. 영상·5개 캡처를 보존했다. 실제 사진 갤러리·OS 이미지 paste는 실행하지 않아 UNVERIFIED이며, 버튼 미구현 판정과 구분한다. 모바일 UI를 통한 실제 provider skill 실행과 iOS 실행 역시 미검증이다. 자세한 결과는 `mobile/report.md`, `mobile/final-result.json`에 있다.

실제 데스크톱 Codex는 격리 저장소 신뢰 확인을 거친 후 hook 검토에서는 `Continue without trusting`을 선택했다. 정상 prompt에서 `inputState:ready` 및 보내기 활성화를 확인한 뒤 `$qa-composer-probe`를 보냈다. `real-codex-ui/result.json`과 캡처에 첫 실패를 보존했다. 고정 대기 시간이나 native 자동완성이 원인이라는 추측은 확정하지 않았다.

후속 검사에서 `CLI 조작 → 선택 / 확인`을 한 번 더 누르면 요청에 테스트 SKILL 본문이 포함되고 응답·대화 기록도 나타났다. 이어 같은 실제 Codex pane에서 일반 텍스트는 보내기 한 번으로 요청·응답이 도착했다. 모든 메시지 전송이 고장난 것은 아니다. skill 입력은 추가 확인이 필요한 사례로 재현했으며, 새 세션의 첫 입력과 skill 문법 중 어느 조건 때문에 발생했는지는 분리하지 않았다. `real-codex-ui/summary.json`에 compact 판정을 남겼다. Claude는 CLI 단독 skill 해석을 확인했지만 실제 interactive 앱 경로까지 검증하지 않았다.

소유 데스크톱 fixture·로컬 provider·emulator-5590을 종료했다. 사용자 앱·다른 emulator는 건드리지 않았다. 데스크톱 원본 WebM은 종료 footer가 없어 lossless remux로 복구한 `desktop-session-recovered.webm`을 제공한다. 15,751개 프레임/630.04초는 복구했지만 종료 시점의 모든 프레임 보존을 보장하지 않는다. 최종 native 결과의 근거는 별도 캡처·JSON이며 원본과 복구 로그는 로컬에 보존했다.

## 재현 근거

- `desktop-ui/result.json`: paste/drop/자동완성/문자 전송과 raw PTY delta
- `desktop-runtime/pane-chat-result.json`: 모델 변경·여러 pane·멀티라인·만료 target 차단 기본 회귀 확인
- `provider/codex-skills-list.json`: 실제 Codex의 격리 skill discovery
- `provider/*-local-skill-probe.json`: 실제 CLI 호출 인자, 종료 코드, 요청에 skill marker 포함 여부
- `provider/*-local-provider-requests.json`: 로컬 provider에 실제 도착한 payload
- `provider/image-history-extraction.json`: 공유 파서 3개 content 입력 실행 결과
- `real-codex-ui/result.json`: 실제 interactive Codex에서 한 번 전송했을 때 요청이 시작되지 않은 재현
- 재현 명령: `pnpm exec tsx qa-results/roky-harness/chat-composer-capabilities/provider/image-history-probe.mjs` → exit0. 진단 실행 성공이며 이미지 기능 PASS를 뜻하지 않는다.

이번 QA는 기능 검증이며 제품 diff hunk 0, unmapped 0, gate-enabling-repair 0, tooling-enforced-format 0이다. 구현 수정·새 테스트 제품 파일·소스 커밋 없음. 전체 CI를 이번 QA에서 다시 실행한 것으로 보고하지 않는다.

진행 기준: [roky-harness-skill](/Users/rookedsysc/.codex/skills/roky-harness-skill/SKILL.md)의 QA 역할. QA는 기본적으로 수정을 구현하지 않으므로 이번 요청에서는 실패 근거를 남기고 제품 기능은 변경하지 않았다.
