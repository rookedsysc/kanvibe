---
title: Paseo와 KanVibe의 Chat UI는 어떻게 동작하는가
reader: 독자: 개발 경험은 있지만 두 앱의 채팅 내부 구조는 처음 보는 사람
---

<summary-box>
Paseo는 데몬이 관리하는 에이전트와 대화한다. KanVibe의 Pane Chat은 이미 열린 터미널 pane에 연결한다.

- Paseo는 `agentId`로 프롬프트를 보낸다. 데몬이 에이전트 이벤트를 타임라인으로 전달한다.
- KanVibe는 pane의 실행 프로세스와 대화 기록을 연결한다. 입력은 CLI 또는 확인된 Codex 스레드에 보내고, 답변은 연결한 기록에서 읽는다.
- KanVibe의 대화 기록 화면은 별도 조회 화면이다. 현재 GPUI 작업 브랜치에서 연결된 화면도 이 대화 기록 화면이다.
</summary-box>

## 구조 {#structure toc="구조"}

같은 Claude Code를 써도 채팅이 붙는 대상이 다르다.
Paseo 앱은 데몬과 통신한다.
KanVibe의 Pane Chat은 터미널 pane과 공급자 기록을 함께 사용한다.
공급자는 Claude Code나 Codex처럼 프롬프트를 받아 실행하는 도구다.
CLI는 터미널에서 실행하는 명령줄 프로그램이다.

<figure src="diagrams/structure.svg">
Paseo는 데몬을 통해 입력과 응답을 교환한다. KanVibe는 터미널 pane의 실행 대상과 공급자 기록을 대조한다.
</figure>

<row-header-table>
| 화면에서 하는 일 | Paseo | KanVibe |
| --- | --- | --- |
| 대화 열기 | 데몬에 등록된 에이전트를 연다. | Pane Chat은 태스크의 터미널 pane에 붙는다. 대화 기록 화면에서는 기록 목록에서 세션을 고른다. |
| 보내기 | 데몬에 `agentId`, 메시지 ID, 프롬프트를 보낸다. | 현재 대상의 `targetToken`을 보낸다. 서비스가 그 대상으로 보낼 수 있는지 재확인한다. |
| 답변 표시 | 데몬이 보낸 텍스트 변화, 도구 이벤트와 타임라인 항목을 표시한다. | 서비스가 공급자 기록을 다시 읽어 메시지로 표시한다. 터미널 화면도 읽어서 CLI 선택창과 입력 상태를 확인한다. |
| 이전 기록과 복구 | 같은 에이전트 채팅에서 과거 턴을 읽는다. 실시간 이벤트가 빠졌으면 데몬 타임라인을 다시 조회한다. | Pane Chat은 연결된 대화의 이전 메시지를 조회한다. 대화 기록 화면은 여러 공급자의 세션을 검색한다. |
</row-header-table>

## 흐름 {#flow toc="흐름"}

두 화면에서 "테스트 실행해줘"를 보낸다고 가정한다.
각 그림의 번호를 누르면 해당 단계가 강조된다.

<flow src="diagrams/paseo-sequence.svg">
Paseo 앱은 에이전트에 프롬프트를 보내고 같은 에이전트의 이벤트를 받는다.

1. 채팅 앱이 `agentId`와 메시지 ID를 데몬에 보낸다. [Paseo의 전송 대상](#paseo-send)
2. 데몬이 공급자 연결을 통해 프롬프트를 실행한다. 채팅 앱에 수락이나 거절을 알린다. [Paseo의 전송 결과](#paseo-send)
3. 데몬이 공급자 이벤트를 기록한다. 채팅 앱에 변경을 보내면 앱이 답변과 도구 행을 갱신한다. [Paseo의 답변 표시](#paseo-read)
4. 실시간 이벤트가 빠졌으면 채팅 앱이 마지막 순번 이후의 타임라인을 가져온다. [Paseo의 기록 복구](#paseo-recover)
</flow>

<flow src="diagrams/kanvibe-sequence.svg">
KanVibe Pane Chat은 pane에 연결할 때 확인한 대상에 입력을 보낸다. 답변은 공급자 기록에서 가져온다.

1. Pane Chat 서비스가 터미널 pane의 실행 프로세스와 대화 기록을 연결한다. [KanVibe의 세션 연결](#kanvibe-bind)
2. Pane Chat 서비스가 `targetToken`으로 전송 대상을 다시 확인한다. 대상이 바뀌면 요청을 거절한다. [KanVibe의 전송 확인](#kanvibe-send)
3. 대상이 맞으면 서비스가 터미널에 붙여넣기와 Enter를 보낸다. 조건을 충족한 Codex 입력은 스레드 API로 보낸다. [KanVibe의 전송 경로](#kanvibe-send)
4. Pane Chat 서비스가 공급자 기록을 다시 읽는다. 채팅 UI가 변경된 메시지를 표시한다. [KanVibe의 답변 표시](#kanvibe-read)
</flow>

## 상세 설명 {#detail toc="상세 설명"}

### Paseo는 무엇을 기준으로 전송하는가 {#paseo-send back="flow"}

Paseo의 전송 대상은 데몬이 관리하는 에이전트다.
에이전트 ID는 그 실행 대상을 지정한다.
채팅 앱의 `dispatchComposerAgentMessage`는 메시지 ID를 만든다.
앱은 사용자 메시지를 먼저 표시하고 `send_agent_message_request`를 보낸다.
데몬은 `agentId`를 해석하고 `sendPromptToAgent`로 공급자에 프롬프트를 넘긴다.
예를 들어 Claude 연결은 Claude Agent SDK를 사용한다.
앱은 데몬의 수락 또는 거절 응답을 받아 사용자 메시지의 전송 상태를 정리한다.
수락 응답과 답변 완료는 시점이 다르다.

### Paseo의 답변은 언제 화면에 나타나는가 {#paseo-read back="flow"}

Paseo는 공급자 이벤트를 받는 동안 답변을 갱신한다.
데몬은 이벤트에 순번을 붙이고 `agent_stream`으로 보낸다.
타임라인은 연속으로 들어오는 답변 조각을 하나의 답변 행에 합친다.
같은 도구 호출의 시작과 완료도 한 항목의 상태로 합친다.
채팅 앱은 저장된 기록과 진행 중인 최신 항목을 한 화면에 배치한다.
사용자 입력, AI 답변, 추론, 도구 실행은 각각에 맞는 행으로 표시한다.

### Paseo는 연결이 끊겨도 어떻게 이어서 읽는가 {#paseo-recover back="flow"}

Paseo 앱은 마지막으로 받은 타임라인 위치를 데몬과 대조한다.
위치에는 타임라인 세대를 나타내는 `epoch`와 순번 `seq`가 들어간다.
데몬이 이미 최신이라고 응답하면 앱은 저장된 내용을 계속 표시한다.
누락된 항목이 있으면 앱은 `fetch_agent_timeline_request`로 이후 항목을 조회한다.
더 최신인 페이지가 남아 있으면 다음 페이지도 가져온다.
이전 대화는 같은 화면에서 과거 방향으로 읽을 수 있다.
데몬이 에이전트를 재개할 때는 공급자가 보존한 기록으로 타임라인을 다시 만든다.

### KanVibe는 어떤 대화가 pane에 열려 있는지 어떻게 아는가 {#kanvibe-bind back="flow"}

Pane Chat은 터미널 pane의 현재 실행 프로세스를 조사하고 그 프로세스가 연 대화 기록을 찾는다.
Claude Code에서는 프로세스의 세션 정보나 이미 확인한 연결이 근거가 될 수 있다.
Codex에서는 화면에 표시된 스레드 ID로 그 스레드의 기록을 찾는 경로도 있다.
같은 작업 폴더에 여러 기록이 있더라도 기록의 최신 시각만으로 전송 대상을 고르지 않는다.

정확한 기록을 연결할 수 있으면 서비스가 대상 토큰인 `targetToken`을 발급한다.
서비스는 전송 직전에 pane, 프로세스, 대화의 일치 여부를 확인한다.
연결에 실패하면 전송을 비활성화한다.
읽을 수 있는 CLI 화면이 있으면 화면 내용을 대신 표시할 수 있다.

### Codex가 열릴 때 훅으로 세션 ID를 보내면 해결되는가 {#codex-session-start-hook toc="Codex 세션 시작 훅"}

`SessionStart` 훅은 첫 입력에 의존하지 않고 ID를 얻을 후보다.
Codex는 훅에 `session_id`와 `cwd`를 전달한다.
현재 KanVibe는 `UserPromptSubmit` 등에서 이미 이 값을 받으며, 시작 훅은 설치하지 않는다.
기존 수신 코드는 훅의 조상 프로세스를 찾아 실제 Codex 실행의 PID와 시작 시각에 ID를 묶는다. [Codex 훅 필드](https://learn.chatgpt.com/docs/hooks#common-input-fields), [KanVibe 훅 등록](https://github.com/rookedsysc/kanvibe/blob/eba089449e671ffcb68b460d43313748a736a2dc/src/lib/codexHooksSetup.ts#L138-L163), [KanVibe의 ID 수신과 연결](https://github.com/rookedsysc/kanvibe/blob/eba089449e671ffcb68b460d43313748a736a2dc/src/lib/aiSessions/paneChatSessionBindings.ts#L48-L105).

**공유 app-server에서는 ID만 보내도 어느 pane인지 알 수 없다.**
예를 들어 같은 작업 폴더에서 pane A와 B가 서로 다른 대화를 열 수 있다.
공유 app-server가 훅을 실행하면 두 훅 모두 같은 서버의 자식이다.
KanVibe는 공유 app-server를 조상으로 찾으면 그 정보를 pane 연결에 사용하지 않는다.
"대화 ID 표시"는 해당 pane의 Codex 상태줄에서 ID를 읽는 기존 경로다. [공유 app-server에서 확인한 동작과 대화 ID 표시](https://github.com/rookedsysc/kanvibe/pull/483), [조상 프로세스 판정 코드](https://github.com/rookedsysc/kanvibe/blob/eba089449e671ffcb68b460d43313748a736a2dc/src/lib/aiSessions/paneChatSessionBindings.ts#L48-L68).

KanVibe가 직접 시작하는 pane이라면 `codex --no-daemon`과 `SessionStart`를 함께 검증할 수 있다.
새 대화와 재개한 대화에서 첫 프롬프트 전에 훅이 도착하는지 확인해야 한다.
이때 훅의 조상 프로세스가 해당 pane의 Codex와 일치해야 한다.
새 대화의 JSONL 파일이 아직 없으면, ID를 받아도 현재 훅 조회는 기록을 연결하지 못한다.
별도의 빈 대화 조회 경로는 이미 있어서, 같은 실행의 대화라고 확인할 수 있을 때 연결에 재사용할 수 있다. [현재 훅의 기록 조회](https://github.com/rookedsysc/kanvibe/blob/eba089449e671ffcb68b460d43313748a736a2dc/src/lib/aiSessions/paneChatSessionBindings.ts#L112-L126), [Codex 빈 대화 조회](https://github.com/rookedsysc/kanvibe/blob/eba089449e671ffcb68b460d43313748a736a2dc/src/desktop/main/services/paneChatService.ts#L345-L378).

현재 호스트의 `codex --version`은 `0.172.0`이다.
이 작업 브랜치의 "대화 ID 표시" 버전 목록에는 `0.169.0`만 있다.
문제가 난 pane도 `0.172.0`을 쓰고 동일한 KanVibe 코드가 실행 중이라면, 그 기능으로 대화 ID를 읽는 경로는 열리지 않는다.
Electron의 자동 전환은 CLI를 찾은 뒤 전송이 가능할 때 진행한다.
Codex가 대화 ID 표시를 기다리는 상태라면 화면을 먼저 열 수 있다.
문제가 난 pane의 Codex 버전과 실패 원인은 ==확인 안 됨==. [Codex 상태줄의 버전 목록](https://github.com/rookedsysc/kanvibe/blob/eba089449e671ffcb68b460d43313748a736a2dc/src/lib/aiSessions/paneChatCodexDisplay.ts#L1-L8), [버전을 확인하는 대화 ID 표시](https://github.com/rookedsysc/kanvibe/blob/eba089449e671ffcb68b460d43313748a736a2dc/src/desktop/main/services/paneChatService.ts#L873-L884), [Electron의 자동 전환 조건](https://github.com/rookedsysc/kanvibe/blob/eba089449e671ffcb68b460d43313748a736a2dc/src/desktop/renderer/components/TerminalPaneChatLayer.tsx#L16-L23).

### KanVibe는 항상 터미널에 타이핑하는가 {#kanvibe-send back="flow"}

KanVibe에는 터미널 입력과 Codex 스레드 API의 두 전송 경로가 있다.

- 터미널 입력 경로는 준비된 입력란에 프롬프트를 붙여넣고 Enter를 따로 보낸다. Claude Code에서는 Enter 직전에 해당 입력란에 붙여넣은 내용이 남아 있는지 확인한다.
- Codex 화면 연동이 활성화되어 있고 대화가 확인되면 일반 프롬프트를 해당 스레드의 Codex app-server API로 보낸다. 내장 슬래시 명령은 터미널 입력 경로를 쓴다.

pane에서 다른 대화로 이동하면 이전에 받은 `targetToken`으로 보낸 요청은 대상 변경으로 거절될 수 있다.
쓰기 시도 뒤 결과가 불명확한 경우도 서비스가 별도로 다룬다.
그래서 전송 오류가 표시됐다는 사실만으로 CLI에 입력이 도달했는지 판단할 수 없다.

### KanVibe는 무엇을 읽어 말풍선을 만드는가 {#kanvibe-read back="flow"}

Pane Chat은 공급자 기록에서 프롬프트와 답변을 읽는다.
터미널 화면은 선택창이나 입력란처럼 현재 CLI 상태를 판별하는 데 쓴다.
예를 들어 Claude의 질문 선택창이 뜨면 Pane Chat은 화면을 해석해 선택지를 보여 줄 수 있다.

Electron 서비스는 구독 중에 기록을 주기적으로 다시 읽는다.
로컬 기록 파일을 감시할 수 있으면 변경 알림도 이용한다.
모바일은 호스트에서 받은 `pane-chat-rows`를 적용한다.
순서가 어긋나면 모바일 컨트롤러가 스냅샷을 다시 요청한다.
따라서 KanVibe의 화면도 실행 중에 갱신된다.
답변의 표시 시점은 공급자가 읽을 수 있는 기록을 남기는 시점에도 영향을 받는다.

### 현재 KanVibe GPUI에서 어떤 화면이 열리는가 {#kanvibe-gpui toc="현재 KanVibe GPUI"}

현재 `perf/rust-gpui` 작업 브랜치에서 대화 기록을 열면 `InlineAiChatView`가 첫 기록 목록을 조회한다.
왼쪽에서 공급자나 검색어로 세션을 찾고, 오른쪽에서 선택한 세션의 메시지를 읽는다.
터미널로 돌아가면 GPUI는 해당 기록 뷰를 버린다.

같은 브랜치의 GPUI 터미널에는 Pane Chat 전환 버튼을 그리는 코드가 있다.
이 버튼에는 채팅 화면을 여는 클릭 핸들러가 연결되어 있지 않다.
Rust 호스트에는 Pane Chat 서비스를 호출하는 경로가 있다.
위의 실시간 Pane Chat 화면 설명은 Electron과 Flutter 모바일의 실제 진입점, 그리고 해당 호스트 서비스 코드에 근거한다.

## 용어 설명 {#glossary toc="용어 설명"}

<term id="daemon" title="데몬(daemon)" aliases="데몬|daemon">
데몬은 호스트에서 실행되는 Paseo 서버 프로세스다.

존재 이유: 데스크톱과 모바일 앱이 같은 실행 대상을 제어할 수 있게 한다.

구체 예: 데몬이 지정된 에이전트에 프롬프트를 전달한다.
</term>

<term id="agent-id" title="에이전트 ID(agentId)" aliases="에이전트 ID|agentId">
에이전트 ID는 Paseo가 관리하는 실행 대상을 가리키는 값이다.

존재 이유: 여러 에이전트 중 프롬프트를 받을 대상을 고른다.

구체 예: Paseo 채팅 앱이 `send_agent_message_request`에 `agentId`를 넣는다.

혼동 주의: Claude Code의 공급자 세션 ID는 별도 값이다.
</term>

<term id="timeline" title="타임라인(timeline)" aliases="타임라인|timeline">
타임라인은 Paseo가 한 에이전트의 대화와 작업 이벤트를 순서대로 모은 데이터다.

존재 이유: 앱이 새 응답을 표시하고 놓친 항목을 다시 읽을 수 있다.

구체 예: 같은 도구 호출의 시작과 완료를 하나의 상태 항목에 반영한다.
</term>

<term id="pane-chat" title="Pane Chat" aliases="Pane Chat">
Pane Chat은 KanVibe에서 실행 중인 터미널 pane의 대화를 조작하는 화면이다.

존재 이유: 터미널에서 열어 둔 CLI 대화를 채팅 화면에서 이어 간다.

구체 예: Electron의 터미널 pane 위에 `PaneChatView`를 연다.

혼동 주의: 대화 기록 화면은 저장된 세션을 찾아 읽는 화면이다.
</term>

<term id="pane" title="터미널 pane(terminal pane)" aliases="터미널 pane|terminal pane">
터미널 pane은 실행 중인 CLI를 표시하고 키 입력을 전달하는 터미널 영역이다.

존재 이유: 한 태스크에 터미널 영역이 여러 개여도 입력 대상을 지정할 수 있다.

구체 예: Claude Code가 열린 pane을 선택하면 그 pane에 Pane Chat을 붙일 수 있다.
</term>

<term id="provider-transcript" title="공급자 기록(provider transcript)" aliases="공급자 기록|provider transcript">
공급자 기록은 에이전트 도구가 대화 내용을 파일이나 자체 저장소에 남긴 데이터다.

존재 이유: 화면에 보이는 글자만으로 알 수 없는 메시지 역할과 순서를 읽을 수 있다.

구체 예: Claude Code의 세션 기록은 JSON Lines 형식으로 저장된다.
</term>

<term id="target-token" title="대상 토큰(targetToken)" aliases="대상 토큰|targetToken">
대상 토큰은 KanVibe가 확인한 실행 대상과 대화를 가리키는 임시 값이다.

존재 이유: 메시지를 작성하는 동안 pane이나 대화가 바뀌었으면 전송을 막을 수 있다.

구체 예: Pane Chat의 프롬프트 요청에 `targetToken`을 넣는다.
</term>

<term id="snapshot" title="스냅샷(snapshot)" aliases="스냅샷|snapshot">
스냅샷은 KanVibe가 한 시점에 읽은 Pane Chat의 상태다.

존재 이유: 앱은 처음 화면을 열 때 대화, 전송 가능 여부, CLI 입력 상태를 함께 받는다.

구체 예: 모바일은 수신한 행의 순서가 어긋나면 새 스냅샷을 요청한다.
</term>

## 출처 {#sources toc="출처"}

Paseo는 로컬 참조 체크아웃 `2a9395e2d1f`의 코드와 공식 타임라인 문서를 기준으로 삼았다.
KanVibe는 현재 `perf/rust-gpui` 작업 브랜치의 소스를 기준으로 삼았다. 기준 커밋은 `f9f09ebcfdc`다.
Electron과 모바일에 연결한 GitHub 파일은 작업 브랜치의 해당 파일과 내용이 같다.
이 비교는 소스에서 실행 경로를 추적한 결과다.

- Paseo의 입력과 수락 처리. [채팅 입력 액션](https://github.com/getpaseo/paseo/blob/2a9395e2d1f7808e478ca4a0b323bf05c33aa45a/packages/app/src/composer/actions.ts#L203-L237), [데몬의 전송 핸들러](https://github.com/getpaseo/paseo/blob/2a9395e2d1f7808e478ca4a0b323bf05c33aa45a/packages/server/src/server/session.ts#L8886-L8997).
- Paseo의 이벤트, 기록 복구와 UI 행. [공식 타임라인 문서](https://github.com/getpaseo/paseo/blob/2a9395e2d1f7808e478ca4a0b323bf05c33aa45a/docs/timeline-sync.md), [채팅 행 렌더러](https://github.com/getpaseo/paseo/blob/2a9395e2d1f7808e478ca4a0b323bf05c33aa45a/packages/app/src/agent-stream/view.tsx#L905-L945).
- KanVibe의 Electron 진입과 대상 확인. [터미널의 Pane Chat 레이어](https://github.com/rookedsysc/kanvibe/blob/eba089449e671ffcb68b460d43313748a736a2dc/src/desktop/renderer/components/TerminalPaneChatLayer.tsx#L266-L284), [대화 연결과 기록 읽기](https://github.com/rookedsysc/kanvibe/blob/eba089449e671ffcb68b460d43313748a736a2dc/src/desktop/main/services/paneChatService.ts#L345-L413), [두 전송 경로](https://github.com/rookedsysc/kanvibe/blob/eba089449e671ffcb68b460d43313748a736a2dc/src/desktop/main/services/paneChatService.ts#L749-L807).
- KanVibe의 기록 갱신. [Electron 구독 서비스](https://github.com/rookedsysc/kanvibe/blob/eba089449e671ffcb68b460d43313748a736a2dc/src/desktop/main/services/paneChatSubscriptionService.ts#L148-L202), [모바일 컨트롤러](https://github.com/rookedsysc/kanvibe/blob/eba089449e671ffcb68b460d43313748a736a2dc/mobile/lib/src/features/pane_chat/application/pane_chat_controller.dart#L244-L274).
- KanVibe Rust 호스트 전송 서비스. `crates/kanvibe-terminal/src/pane_chat/service/send.rs:207`.
- KanVibe GPUI의 대화 기록 진입과 터미널 버튼. 아래 경로는 로컬 작업 브랜치 기준이다. 대화 기록 뷰 생성 `crates/kanvibe-app/src/views/task_detail/mod.rs:366`, 기록 조회 `crates/kanvibe-app/src/views/task_detail/chat_view.rs:90`, 터미널 버튼 렌더러 `crates/kanvibe-app/src/views/task_detail/terminal/pane.rs:673`.
