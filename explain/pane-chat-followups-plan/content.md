---
title: Pane Chat 후속 4건 작업 계획
reader: 독자: KanVibe Pane Chat 코드를 아는 개발자. 이 계획의 범위와 테스트 선택을 승인하려는 사람
profile: prd
---

<summary-box>
Pane Chat 지연 개선에서 남긴 후속 4건을 리서치 실측에 맞춘 범위로 나눴고, 승인 전에 정할 것은 열린 결정 4개와 테스트 선택 12개다.

- 코드가 바뀌는 것은 둘이다. Zellij pane의 root PID를 환경 변수로 찾고(F1), 배치가 그대로이면 다시 그리지 않는다(F3).
- 나머지 둘은 테스트와 fixture만 바꾼다. Herdr 첫 실행 화면을 설정으로 끄고(F2), 신뢰 화면 순서를 시나리오로 고정한다(F4).
- F4는 전제가 재현되지 않았다. Claude 2.1.332는 신뢰를 수락한 0.11초 뒤에 세션 기록을 쓰므로 제품 코드는 바꾸지 않는다.
</summary-box>

## 결정할 것: 범위 4건과 열린 결정, 테스트 선택 {#decide toc="결정할 것"}

이 문서는 계획 파일 `.claude/plan/pane-chat/2611/26-pane-chat-followups.plan.md`를 검토용으로 옮긴 것이다.
정책과 결정의 정본은 계획 파일이고, 이 페이지는 같은 ID와 값을 쓴다.
승인 전에는 코드를 고치지 않는다.

독자가 정할 것은 세 가지다.

- 아래 표의 건별 범위가 맞는지 본다.
- [열린 결정](#open-questions) 네 개에서 선택지를 고른다. 각 항목에 권장안이 있다.
- [테스트 선택](#test-case-decision) 열두 개를 Required 또는 Skip으로 정한다. 하나라도 미정이면 구현을 시작하지 않는다.

<row-header-table>
| 건 | 리서치 결론 | 이번에 하는 일 | 제품 코드 변경 |
| --- | --- | --- | --- |
| [F1 Zellij](#f1) | 가설이 성립한다. pane 번호와 `ZELLIJ_PANE_ID`가 같다 | 서버의 직접 자식 환경을 읽어 root PID를 채운다 | 있음 |
| [F2 Herdr](#f2) | 원인은 `onboarding` 설정 누락이다 | fixture가 설정을 미리 쓰고 서버를 정리한다. 실제 Claude로 검증한다 | 없음 |
| [F3 다시 그리기](#f3) | 무조건 notify가 창 전체를 다시 그린다 | 네 가지 상태가 바뀐 때만 notify한다 | 있음 |
| [F4 신뢰 화면](#f4) | 가리는 경로가 재현되지 않았다 | 현재 순서를 끝단 시나리오와 실제 Claude 확인으로 고정한다 | 없음 |
</row-header-table>

## 건별 범위: 무엇을 고치고 무엇을 남기는가 {#scope toc="건별 범위"}

각 건은 원인, 고른 방법, 범위에서 뺀 것의 순서로 적는다.
수치는 Linux, Zellij 0.46.0, Herdr 0.9.5, Claude Code 2.1.332에서 잰 값이다.

### F1 Zellij: root PID를 pane 환경 변수로 찾는다 {#f1 toc="F1 Zellij"}

Zellij에서 채팅이 자동으로 열리지 않는 이유는 KanVibe가 pane의 root PID를 얻지 못해서다.
`read_zellij_surface`는 `list-panes` JSON의 `terminal_pid`와 `pid`를 읽는데, Zellij는 이 값을 주지 않는다.
root PID가 없으면 CLI 확인이 "CLI 없음"으로 끝나고 자동 열기 확인이 통과하지 못한다.

Zellij 서버는 pane 프로세스를 띄울 때 `ZELLIJ_PANE_ID`를 환경에 넣는다.
격리 서버에서 pane 0, 1, 2, 3, 5의 값이 `list-panes`의 `id`와 모두 같았다.
pane 안에서 띄운 자손도 같은 값을 물려받으므로, 서버의 직접 자식으로 좁혀야 root 하나가 남는다.
아래 그림은 pane 5를 찾는 예다.

<figure src="diagrams/zellij-root-lookup.svg">
KanVibe 조회는 Zellij 서버의 직접 자식 중 세션 이름과 pane 번호가 모두 맞는 프로세스 하나만 root로 고른다. 값을 물려받은 중첩 셸과 Claude CLI는 직접 자식이 아니라서 제외한다.
</figure>

고른 방법은 다음과 같다.

- D1: root는 명령줄에 `--server`가 있는 `zellij` 프로세스의 직접 자식 가운데, 환경의 `ZELLIJ_SESSION_NAME`과 `ZELLIJ_PANE_ID`가 모두 같은 프로세스다. 정확히 하나일 때만 쓴다. 0개나 2개 이상이면 지금처럼 `None`이다.
- D2: 조회는 `/proc`만 읽는 POSIX `sh` 스크립트 하나다. `pgrep`을 쓰면 40.4ms, `/proc`만 읽으면 14.2ms였다. `list-panes` 한 번은 18.9ms다.
- D3: surface를 읽을 때 고른 pane 하나만 조회한다. 1초마다 도는 배치 조회에서는 생략한다. JSON에 PID가 있으면 그 값을 먼저 쓴다.
- D4: 범위는 자동으로 열리고 대화가 보이는 데까지다.

세션 이름으로 서버를 직접 찾지 않는 이유가 있다.
Zellij 0.46.0은 소켓 파일 이름이 세션 이름이 아니라 불투명 ID다.
그래서 서버 명령줄에는 세션 이름이 없고, 자식의 `ZELLIJ_SESSION_NAME`으로 맞춘다.

아래는 조회 스크립트의 뼈대다. 제안이며 아직 구현하지 않았다.

```bash
session=$1
pane=$2
for comm in $(grep -lxF -e zellij /proc/[0-9]*/comm 2>/dev/null); do
  server=${comm#/proc/}
  server=${server%/comm}
  tr '\0' '\n' < "/proc/$server/cmdline" | grep -qxF -e --server || continue
  # 서버의 직접 자식만 읽는다. 자손은 같은 값을 물려받아도 대상이 아니다.
  children=$(cat /proc/$server/task/*/children 2>/dev/null)
  for child in $children; do
    grep -qzxF -e "ZELLIJ_PANE_ID=$pane" "/proc/$child/environ" || continue
    grep -qzxF -e "ZELLIJ_SESSION_NAME=$session" "/proc/$child/environ" || continue
    echo "$child"
  done
done
```

범위에서 뺀 것은 둘이다.

- 전송과 선택 카드. 전송은 입력란 상태가 `Ready`여야 하는데 Zellij surface는 커서와 화면을 읽지 않는다. 채팅은 열리고 대화는 보이지만 보내기는 계속 막힌다. [OQ2](#open-questions)에서 정한다.
- macOS. `/proc`가 없어 스크립트가 아무것도 출력하지 않고 지금 동작을 유지한다. [OQ1](#open-questions)에서 정한다.

검토한 다른 방법은 모두 탈락했다.

| 방법 | 탈락 이유 |
| --- | --- |
| `list-panes -c`, `list-clients`, 세션 메타데이터 | PID가 없다(실측) |
| 플러그인 API `get_pane_pid` | wasm 플러그인 배포와 `ReadApplicationState` 권한 창이 필요하다. Zellij 전역 부작용이라 환경 안전 규칙과 충돌한다 |
| 서버 소켓 직접 통신 | 비공개 프로토콜이다 |

### F2 Herdr: fixture가 첫 실행 화면을 끄고 서버를 정리한다 {#f2 toc="F2 Herdr"}

Herdr 실제 CLI 검증이 막힌 이유는 fixture에 Herdr 설정 파일이 없어서다.
fixture는 `XDG_CONFIG_HOME`을 격리하고, 그 아래 `herdr/config.toml`이 없으면 Herdr가 첫 실행 화면을 띄운다.
격리 세션에서 그 화면은 키 입력을 pane에 넘기지 않았다.
그 화면의 Enter는 통합 설치를 시도했으므로 키 입력으로 넘기는 방식은 쓰지 않는다.

D5로 fixture가 아래 내용을 `xdg/config/herdr/config.toml`에 미리 쓴다.
이 설정으로 띄운 격리 세션에서는 화면이 뜨지 않았고 입력이 pane에 들어갔다.

```text
onboarding = false

[update]
version_check = false
manifest_check = false

[ui.sound]
enabled = false
```

함께 고칠 fixture의 빈틈은 둘이다.

- `multiplexer_bin_dirs()`가 tmux와 zellij만 PATH에 넣는다. herdr를 더한다.
- 종료 처리가 tmux만 끈다. D6으로 fixture 환경 안에서 실행 중인 Herdr 세션을 이름으로 `session stop`, `session delete`한다. 리서치 중에 13시간 남은 고아 서버를 실제로 봤다.

KanVibe의 Herdr 경로가 받는 값은 명령 수준에서 확인했다.
`pane process-info`가 `shell_pid`를 주고, 그 값으로 읽은 프로세스 표에서 가짜 Claude가 판정 조건에 맞았다.
앱을 띄운 끝단 확인은 아직 하지 않았고 이번 계획의 T6과 T12가 맡는다.

화면 기준 이미지는 바뀌지 않는다.
기준 상태 가운데 Herdr task의 터미널을 여는 상태가 없기 때문이다.

### F3 다시 그리기: 상태가 바뀐 때만 알린다 {#f3 toc="F3 다시 그리기"}

터미널 화면을 보는 동안 1초마다 창 전체를 다시 그리는 이유는 배치 조회가 끝날 때마다 무조건 notify하기 때문이다.
GPUI는 notify된 view의 조상을 모두 dirty로 표시하고, 앱에는 캐시된 view가 없다.
다시 그리기 한 번은 약 31ms다.

D7로 `finish_layout`의 시작과 끝에서 네 값을 비교해 달라졌을 때만 notify한다.

| 비교하는 값 | 화면에서 쓰이는 곳 |
| --- | --- |
| `layout` | 채팅 덮개의 위치, 전환 버튼, 연결 화면 |
| `is_layout_failed` | 아래쪽 안내 문구 |
| `is_chat_focused` | 작업 상세가 터미널에 포커스를 줄지 판단 |
| `focused_pane` | 포커스된 pane이 사라졌을 때의 초기화 |

나머지 변화는 이미 각자 notify한다.
포커스 복원과 toggle 완료는 `focus_pane`이, 자동 열기 확인 결과는 `probe_pane`이, 자식 화면의 가시성은 `set_visible`이 알린다.

| 상황 | 지금 | 변경 후 |
| --- | --- | --- |
| 배치가 그대로인 조회 | 조회마다 창 전체를 다시 그린다 | 다시 그리지 않는다 |
| 배치 변경, 조회 실패, 포커스 pane 소멸 | 다시 그린다 | 다시 그린다 |

이 건은 그림 없이 표로 충분하다.
전후 차이가 한 줄이기 때문이다.

### F4 신뢰 화면: 제품 코드는 그대로 두고 순서를 테스트로 고정한다 {#f4 toc="F4 신뢰 화면"}

채팅이 신뢰 화면 위로 열린다는 전제는 재현되지 않았다.
자동 열기는 대화 연결이 필요하고, 연결 증거인 세션 기록은 신뢰를 수락한 뒤에 생긴다.
아래 그림은 실제 Claude 2.1.332에서 잰 순서다.

<figure src="diagrams/trust-order.svg">
신뢰 화면이 떠 있는 동안에는 세션 기록이 없어 KanVibe가 채팅을 열지 않는다. 수락 0.11초 뒤 세션 기록이 생기면 KanVibe가 그 신호로 다시 확인하고 채팅을 연다.
</figure>

실측은 두 조건에서 했다.
일반 폴더에서는 25초, hook을 선언한 git 저장소에서는 36초 동안 신뢰 화면을 띄워 뒀고 그동안 세션 기록이 없었다.
SessionStart hook도 수락 뒤에만 실행됐다.

화면 자체도 선택 카드로 읽히지 않는다.
선택지에 번호가 없고 기본 강조가 `No, exit`다.
저장소의 `read_claude_prompt`를 그대로 복사해 이 화면에 돌린 결과는 `null`이었다.

그래서 D8로 제품 코드는 바꾸지 않는다.
대신 두 가지로 순서를 고정한다.

- 가짜 Claude에 신뢰 대기 모드를 넣는다. 신뢰 화면 글을 그리고 Enter 전에는 세션 기록을 쓰지 않는다. 그동안 채팅이 열리지 않는지 끝단에서 본다(T10).
- 실제 Claude로 같은 순서를 1회 확인한다(T12). Claude 버전을 올릴 때 다시 돌릴 항목이다.

범위에서 뺀 것은 둘이다.

- 신뢰 질문을 채팅 카드로 보여 주는 안. 번호가 없어 숫자 키로 고를 수 없고, 기본값이 `No, exit`라 잘못 보내면 CLI가 종료된다.
- 신뢰 화면 문구를 읽어 자동 열기를 막는 방어 코드. 재현되지 않은 상황을 위한 코드다. [OQ3](#open-questions)에서 정한다.

## 열린 결정: 승인 전에 정할 네 가지 {#open-questions toc="열린 결정"}

네 항목 모두 범위를 바꾸는 선택이라 계획이 임의로 정하지 않았다.

<row-header-table>
| ID | 질문 | A (권장) | B | 판단 기준 |
| --- | --- | --- | --- | --- |
| OQ1 | macOS에서 Zellij root 조회를 이번에 넣을까 | Linux와 Linux 원격만 한다. macOS는 지금 동작을 유지한다 | `ps -E` 기반 분기를 함께 넣는다. Mac에서 확인할 사람이 필요하다 | Mac에서 `ps -E`가 pane 셸의 환경을 보여 주는가. 이 머신은 Linux라 실측할 수 없다 |
| OQ2 | Zellij에서 전송과 선택 카드까지 열까 | 이번 범위 밖으로 둔다 | 커서와 화면 캡처를 함께 한다. Todo와 실제 CLI 검증이 늘어난다 | 이번 목표가 자동 연결까지인가, 전송까지인가 |
| OQ3 | 신뢰 화면 문구를 읽는 방어 코드를 지금 넣을까 | 넣지 않는다. 신뢰 전에 세션 기록을 쓰는 Claude 버전이 확인되면 넣는다 | 지금 넣는다. 자동 열기 확인이 이미 읽는 화면에서 문구와 커서를 본다 | 재현되지 않은 상황에 코드를 둘 것인가 |
| OQ4 | 실제 CLI 시나리오 드라이버를 저장소에 남길까 | `parity/tools/perf/pane_chat_real_cli.py`로 남긴다. 세 시나리오가 쓴다 | 임시 스크립트로 1회만 돌리고 결과만 남긴다 | Claude 버전을 올릴 때 다시 돌릴 것인가 |
</row-header-table>

## 테스트 선택: Required와 Skip을 정할 열두 항목 {#test-case-decision toc="테스트 선택"}

각 행은 입력과 기대 결과를 적은 계약이다.
결정 칸은 모두 미정이며 권장값만 적었다.

<row-header-table>
| ID | 수준 | 입력 | 기대 | 지금 RED인 이유 | 권장 |
| --- | --- | --- | --- | --- | --- |
| T1 | 단위 (F1) | Zellij 조회 결과가 고른 pane에 PID 한 개를 준다 | `root_pid = Some(pid)` | JSON의 PID만 읽어 항상 `None`이다 | Required |
| T2 | 단위 (F1) | 조회 결과가 0줄, 2줄 이상, 숫자 아님, 명령 실패 | `root_pid = None`, surface와 화면 조회는 성공 | T1의 최소 구현은 2줄일 때 값을 돌려준다 | Required |
| T3 | 통합 (F1) | 실제 `sh` 조회 스크립트와 가짜 서버의 자식, 손자 | 두 변수가 맞는 직접 자식의 PID 한 줄만 출력 | 스크립트가 없다 | Required |
| T4 | 회귀 (F1) | Zellij task의 배치 조회 1회 | 실행 명령에 root 조회가 없다 | 공통 경로에 넣으면 배치 조회도 조회한다 | Required |
| T5 | 끝단 (F1) | fixture Zellij task에서 가짜 Claude 실행 | 30초 안에 `pane_chat:ready` 표식 | 이전 실행이 30초 시간 초과다 | Required |
| T6 | 끝단 (F2) | fixture Herdr task에서 가짜 Claude 실행 | 첫 실행 화면이 없고 `pane_chat:ready` 표식 | 화면이 입력을 가로채 CLI가 시작되지 않는다 | Required |
| T7 | 회귀 (F2) | Herdr를 쓴 fixture 실행의 종료 | 그 fixture의 `herdr` 프로세스 0개 | 종료 처리가 tmux만 끈다 | Required |
| T8 | 단위 (F3) | 같은 배치를 돌려주는 조회 3회 | 층의 notify 0회 | 조회마다 notify한다 | Required |
| T9 | 회귀 (F3) | 활성 pane 변경, 좌표 변경, 조회 실패, 복구, 포커스 pane 소멸 | notify가 오고 덮개, 안내, 포커스 상태가 맞다 | 없음. 변경 전후 모두 GREEN이어야 하는 보호 테스트다 | Required |
| T10 | 끝단 (F4) | 가짜 Claude 신뢰 대기 모드 | 신뢰 화면 동안 `pane_chat:open` 없음, Enter 뒤 `pane_chat:ready` | 가짜에 신뢰 대기 모드가 없다 | Required |
| T11 | 단위 (F4) | 2.1.332 신뢰 화면의 줄과 커서 | `read_claude_prompt`가 `None` | 없음. 이미 GREEN인 특성 고정이다 | Skip |
| T12 | 수동 (공통) | 실제 Claude haiku 3종 | Zellij와 Herdr는 자동으로 열리고, 신뢰 화면 10초 동안은 열리지 않는다 | 해당 없음 | Required |
</row-header-table>

테스트 방법에 열린 질문이 하나 있다.

- OTQ1 (F3): "유휴 상태에서 다시 그리지 않는다"를 끝단에서 무엇으로 판정할지 정해야 한다. 지금 성능 표식에는 일반 프레임마다 나오는 표식이 없다. A는 T8의 notify 횟수와 기존 구간 10회 재측정이다. B는 측정 도구에 유휴 프레임 수 표식을 추가하는 것이다. C는 A에 수동 CPU 관찰을 더하는 것이다. 권장은 A다.

## 작업 순서: Todo 열한 개와 의존 관계 {#todos toc="작업 순서"}

단계 1은 서로 독립이라 함께 시작할 수 있다.
Required로 정한 테스트는 해당 Todo에서 실패를 먼저 확인한 뒤 구현한다.

<row-header-table>
| Todo | 단계 | 선행 | 하는 일 | 검증 |
| --- | --- | --- | --- | --- |
| 1 | 1 | 없음 | Zellij root 조회 스크립트와 결과 해석 함수(`surface/zellij_root.rs`) | T2, T3 |
| 2 | 2 | 1 | `read_zellij_surface`에 연결, 배치 조회에서는 생략 | T1, T4, parity 장부 |
| 3 | 3 | 2 | 측정 드라이버에 `--task` 추가, Zellij 끝단 실행 | T5 |
| 4 | 1 | 없음 | fixture의 Herdr 설정, PATH, 종료 정리 | T7 |
| 5 | 3 | 3, 4 | Herdr 끝단 실행 | T6, T7 |
| 6 | 1 | 없음 | `finish_layout`의 notify를 상태 변화에 묶기 | T8, T9 |
| 7 | 2 | 6 | 기존 구간 10회 재측정 | 이전 값의 5% 이내 |
| 8 | 1 | 없음 | 가짜 Claude 신뢰 대기 모드와 끝단 시나리오 | T10, 선택 시 T11 |
| 9 | 2 | 3 | 실제 CLI 시나리오 드라이버(OQ4가 A일 때) | 신뢰 순서 시나리오 선실행 |
| 10 | 4 | 2, 4, 9 | 실제 Claude 3종 실행과 기록 | T12 |
| 11 | 5 | 전부 | fmt, clippy, nextest, parity, 릴리스 빌드와 보고서 갱신 | 게이트 5종 통과 |
</row-header-table>

## 적용 판단: 이 문서의 깊이와 변경 요약 {#detail toc="적용 판단"}

이 문서는 구현 계약 전체가 아니라 승인을 위한 결정 요약이다.
구현에 쓰는 파일별 작업과 RED, GREEN 기록 형식은 계획 파일의 Todo에 있다.

| PRD 프로파일 | 이유 | 플랫폼 | 문서 상태 | 코드 상태 |
| --- | --- | --- | --- | --- |
| compact | 새 기능이 아니라 기존 Pane Chat의 결함 수정, 검증 환경, 성능 후속 4건의 범위와 테스트 선택을 승인받는 결정 요약이다 | 없음 | draft | proposed |

| 목표 | 변경 범위 | 영향 | 확인 방법 |
| --- | --- | --- | --- |
| Zellij에서도 채팅이 자동으로 열리게 하고, Herdr 실제 CLI 검증을 가능하게 하고, 유휴 다시 그리기를 없애고, 신뢰 화면 순서를 테스트로 고정한다 | `read_zellij_surface`의 root PID 조회 추가, fixture의 Herdr 설정과 정리, `finish_layout`의 알림 조건, 가짜 Claude 신뢰 대기 모드와 실제 CLI 시나리오 | Zellij task에서 채팅이 자동으로 열린다(전송은 계속 막힌다). 터미널 화면의 1초 주기 다시 그리기가 사라진다. F2와 F4는 테스트와 fixture만 바뀐다 | cargo 게이트 5종, fixture 끝단 시나리오 3종(T5, T6, T10), 실제 Claude haiku 3종(T12) |

이번 계획이 다루지 않는 것은 다음과 같다.

- Codex 연결 방식 전부(ID 추정, 0.169.0 허용 목록 확대, 실행 옵션 래퍼, `-c`, `--remote`, app-server 프록시).
- 리서치 중 발견한 고아 Herdr 서버(PID 3610626)와 `~/.claude.json`의 임시 폴더 신뢰 항목 정리.

## 남는 위험: 이번에 검증하지 못하는 것 {#risks toc="남는 위험"}

계획대로 끝내도 아래는 확인되지 않은 채 남는다.

- macOS. Zellij root 조회가 동작하지 않고 지금 동작을 유지한다. OQ1이 B가 되어도 Mac 실측이 필요하다.
- SSH 원격. 이 머신에서 localhost SSH 인증이 되지 않아 원격 실행을 재지 못했다. 근거는 기존 프로세스 표 스크립트와 같은 전달 방식이라는 점뿐이다.
- 세션 이름을 바꾼 Zellij 세션. 기존 pane은 옛 이름을 유지하므로 조회가 실패하고 지금처럼 연결되지 않는다. 잘못된 대상에 연결하지는 않는다.
- 래퍼로 실행한 Zellij. 프로세스 이름이 `zellij`가 아니면 서버를 찾지 못한다. 역시 연결되지 않는 쪽으로 실패한다.
- F3의 암묵적 의존. 다른 화면이 1초 다시 그리기에 기대고 있었다면 갱신이 멈춘다. 그런 의존은 찾지 못했고 화면 기준 캡처로 확인한다.
- Claude의 버전 변화. 세션 기록을 쓰는 시점이 신뢰 전으로 당겨지면 F4의 전제가 바뀐다. T12를 버전마다 다시 돌려 잡는다.

## 용어 {#glossary toc="용어"}

<term id="root-pid" title="root PID" aliases="root PID">
root PID는 터미널 pane 하나에서 가장 먼저 뜬 프로세스의 번호다.

존재 이유: KanVibe는 이 번호에서 시작해 그 pane에서 도는 CLI를 찾는다. 번호가 없으면 CLI가 없다고 판정한다.

구체 예: Zellij pane 5의 root는 서버가 직접 띄운 셸이다. 실측에서 PID는 2380671이었다.
</term>

<term id="direct-child" title="직접 자식" aliases="직접 자식">
직접 자식은 어떤 프로세스가 스스로 띄운 프로세스다.

존재 이유: 환경 변수는 자손에게 그대로 내려간다. 직접 자식으로 좁히지 않으면 같은 pane 번호를 가진 프로세스가 여럿 잡힌다.

구체 예: pane 5에서 셸, 중첩 셸, 가짜 Claude가 모두 `ZELLIJ_PANE_ID=5`였다. 서버의 직접 자식은 셸 하나였다.
</term>

<term id="surface" title="surface" aliases="surface">
surface는 KanVibe가 pane 하나를 읽은 결과다.

존재 이유: 어느 pane인지, 그 pane의 root가 무엇인지, 입력란이 어떤 상태인지를 한 값으로 넘기기 위해서다.

구체 예: Zellij surface는 지금 pane 식별자만 채우고 root와 커서는 비어 있다.
</term>

<term id="session-record" title="세션 기록" aliases="세션 기록">
세션 기록은 Claude Code가 실행 중인 대화의 ID를 적어 두는 파일이다.

존재 이유: KanVibe는 이 파일로 pane의 Claude 프로세스와 대화를 잇는다. 파일이 없으면 연결할 증거가 없다.

구체 예: 경로는 `~/.claude/sessions/2455754.json` 같은 모양이고 숫자는 Claude의 PID다.
</term>

<term id="auto-open" title="자동 열기" aliases="자동 열기">
자동 열기는 사용자가 누르지 않아도 pane 위에 채팅을 여는 동작이다.

존재 이유: 기본 채팅 모드나 저장된 채팅 모드를 쓰는 사용자가 매번 전환하지 않게 하려는 것이다.

구체 예: 그 pane의 CLI와 대화를 연결할 수 있는 것이 확인된 뒤에만 연다. 확인 전에는 터미널을 그대로 보인다.
</term>

<term id="trust-screen" title="신뢰 화면" aliases="신뢰 화면">
신뢰 화면은 Claude Code가 처음 쓰는 폴더에서 그 폴더를 믿는지 묻는 화면이다.

존재 이유: 폴더의 설정과 hook이 명령을 실행할 수 있어서, 사용자가 먼저 동의해야 한다.

구체 예: 2.1.332의 선택지는 `No, exit`와 `Yes, I trust this folder`이고 번호가 없다.
</term>

<term id="notify" title="notify" aliases="notify">
notify는 GPUI에서 어떤 view의 상태가 바뀌었다고 알리는 호출이다.

존재 이유: GPUI는 이 호출을 받은 view와 그 조상을 다시 그린다.

구체 예: 채팅 층이 notify하면 작업 상세와 창의 루트까지 다시 그려진다. 한 번에 약 31ms가 든다.
</term>

<term id="fixture" title="fixture" aliases="fixture">
fixture는 테스트용으로 만든 격리된 앱 환경이다.

존재 이유: 사용자의 실제 HOME, 설정, 터미널 세션을 건드리지 않고 앱을 띄우기 위해서다.

구체 예: fixture는 `HOME`과 `XDG_CONFIG_HOME`을 임시 폴더로 바꾼다. 그래서 Herdr가 설정이 없는 첫 실행으로 뜬다.
</term>

## 출처 {#sources toc="출처"}

코드와 실측 근거는 2026-11-26 리서치에서 얻었다.

- Zellij가 pane 프로세스에 `ZELLIJ_PANE_ID`를 넣는 코드: [os_input_output_unix.rs#L247-L249](https://github.com/zellij-org/zellij/blob/aedee21cb0d58d716496e7f6c66ba0b6f08045d8/zellij-server/src/os_input_output_unix.rs#L247-L249)
- `list-panes` 항목에 PID가 없는 구조체: [data.rs#L3598-L3608](https://github.com/zellij-org/zellij/blob/aedee21cb0d58d716496e7f6c66ba0b6f08045d8/zellij-utils/src/data.rs#L3598-L3608)
- 소켓 이름이 세션 ID라는 설명: [lib.rs#L3772-L3773](https://github.com/zellij-org/zellij/blob/aedee21cb0d58d716496e7f6c66ba0b6f08045d8/zellij-server/src/lib.rs#L3772-L3773)
- 플러그인 API의 PID 조회와 필요한 권한: [shim.rs#L2467-L2468](https://github.com/zellij-org/zellij/blob/aedee21cb0d58d716496e7f6c66ba0b6f08045d8/zellij-tile/src/shim.rs#L2467-L2468), [zellij_exports.rs#L6883](https://github.com/zellij-org/zellij/blob/aedee21cb0d58d716496e7f6c66ba0b6f08045d8/zellij-server/src/plugins/zellij_exports.rs#L6883)
- Herdr 첫 실행 설정: `herdr --default-config`의 `onboarding` 주석(0.9.5)
- KanVibe 코드: `crates/kanvibe-terminal/src/pane_chat/surface.rs`, `service/target.rs`, `service/reading.rs`, `crates/kanvibe-app/src/views/task_detail/pane_chat/layer.rs`, `crates/kanvibe-ai/src/pane_chat/claude_prompt.rs`
- GPUI notify 동작: gpui-fast 0.1.19의 `app.rs`와 `window.rs`
- 이전 측정과 실제 CLI 결과: `qa-results/roky-harness/pane-chat-latency/report.md`
- 계획 정본: `.claude/plan/pane-chat/2611/26-pane-chat-followups.plan.md`
