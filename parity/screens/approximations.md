# 기준 이미지의 근사 상태

인벤토리(02-screens.md)의 상태 설명과 기준 이미지가 정확히 같지 않은 곳이다. Rust 화면을 맞출 때는 설명이 아니라 이 이미지와 `states.yaml`의 단계를 기준으로 삼는다.

## 근사한 상태 9개

| ID | 인벤토리 설명 | 기준 이미지가 실제로 보여 주는 것 | 이유 |
|---|---|---|---|
| SCR-BOARD-23 | macOS 타이틀바 레이아웃 | 렌더러만 macOS로 판정(`navigator.platform`·`userAgent`를 Mac으로 바꿈)한 레이아웃. 창 테두리는 Linux `titleBarOverlay` 그대로 | 이 머신은 Linux다. Rust 앱은 QA 모드에서 플랫폼을 macOS로 강제하는 장치가 필요하다 |
| SCR-TASK-26 | macOS dock 상단 오프셋 | 위와 같은 방식 | 위와 같음 |
| SCR-TASK-10 | 호출 그래프(계보 선 + 시간축 막대) | 그래프 패널의 빈 상태 "이 세션은 서브에이전트를 띄우지 않았습니다." | fixture 세션 기록에서 서브에이전트 호출 연결을 만들지 못했다 |
| SCR-TASK-16 | 연결 버튼 / 연결 중 / 실패 | "연결 중…" (가운데 문구) | 연결 서비스를 실패로 두어도 TS 앱 화면은 연결 중에 머문다 |
| SCR-TASK-17 | TerminalLoader 로딩 / 탭 목록 로드 실패 | 탭 목록 조회가 끝나지 않아 탭 바 대신 세션 이름이 보이는 터미널 | TS 앱의 로딩 표시는 탭 바가 없는 모습으로만 드러난다 |
| SCR-TASK-19 | composer: 자동완성 / 첨부 칩 / 모델 picker / 승인 프롬프트 | 가짜 Claude CLI(`parity/fixtures/agent-cli/pane-chat-cli.py`)를 pane에서 실행한 뒤 모델 선택 팝오버 | 실제 Claude Code 계정이 필요 없도록 프롬프트 표식과 기록 형식만 흉내 낸 CLI를 쓴다 |
| 고장 주입 오류 문구 전체 | 로드·저장 실패 메시지 | `Error invoking remote method 'kanvibe:invoke': Error: <사유>` | Electron IPC가 오류에 붙이는 머리말이 화면에 그대로 나온다. Rust 앱이 이 문구를 재현할지는 결정이 필요하다 |
| 화면에 찍힌 경로 | 사용자 프로젝트 경로 | `/tmp/kanvibe-parity/fixture/...` | 결정성을 위해 고정 경로를 쓴다. Rust 캡처도 같은 `--root`로 찍어야 한다 |
| SCR-TASK-08 원격 오류 문구 | 원격 hooks 상태 | ssh가 `devbox`를 찾지 못한 오류 문구 | OpenSSH는 `$HOME`이 아니라 passwd의 홈에서 `~/.ssh/config`를 읽는다. 그 설정에 `devbox`가 있으면 문구가 바뀐다 |

## 여러 변형 중 대표로 찍은 것

인벤토리에서 한 ID에 변형이 여러 개인 상태는 하나만 찍었다.

| ID | 찍은 변형 |
|---|---|
| SCR-BOARD-08 | 네 카드에 삭제 중 · 삭제 실패 · 이동 저장 중 · 이동 실패를 한 화면에 함께 |
| SCR-BOARD-09 | Done 더 보기 버튼(로딩 중·실패는 없음) |
| SCR-BOARD-13 | 프로젝트 선택 + 브랜치 검색 드롭다운("feat" 입력). 생성 중·생성 실패는 없음 |
| SCR-BOARD-20 | 알림 목록(전체 필터) + 백그라운드 동기화 대기 상태 |
| SCR-BOARD-21 | 찾기 바 일치("필터") |
| SCR-BOARD-22 | Project 세그먼트 선택 |
| SCR-WS-05 | 자동 연결 중 |
| SCR-WS-06 | 검색 불일치("zzzz") |
| SCR-WS-07 | 정렬 팝오버 |
| SCR-WS-08 | Done 더 보기 버튼 |
| SCR-TASK-06 | 설명 인라인 편집 시작(저장 중·실패 없음) |
| SCR-TASK-08 | 원격 태스크의 미설치 + 연결 실패 |
| SCR-TASK-09 | 실행 중 세션 1개 + 서브태스크 1개 |
| SCR-TASK-11 | 값(Claude·Codex 사용량, Gemini 미로그인) |
| SCR-TASK-12 | Claude 대화(마크다운·코드·표·Mermaid·도구 호출) |
| SCR-TASK-13 | 빈 상태 |
| SCR-TASK-15 | 탭 3개(이름 변경·생성 중·실패·드래그 없음) |
| SCR-TASK-18 | CLI 없음(noAgent) |
| SCR-TASK-20 | VS Code 실행 중 스피너 |
| SCR-TASK-23 | 세션 타입 네이티브 select 펼침 |
| SCR-DIFF-04 | 편집 후 저장 안 됨 |
| SCR-SET-05 | 페어링 코드 발급 + 기기 목록 2개 |
| SCR-SET-06 | 일괄 변경 진행 중 |
| SCR-SHORT-04 | 로드 실패 |
| SCR-AIACC-07 | 계정 추가 실패 |
| SCR-PANE-05 | 4분할 + 명령 두 개 입력 |
| SCR-PANE-06 | 저장 성공 |
| SCR-GLOBAL-01 | 결과 하이라이트("필터") |
| SCR-GLOBAL-02 | 전체 명령 |
| SCR-GLOBAL-06 | 실패 토스트(알림 열기 실패) |
