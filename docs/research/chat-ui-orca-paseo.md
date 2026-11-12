# Orca · Paseo 채팅 UI 분석과 KanVibe 적용 기준

Orca와 Paseo는 사용자 입력을 작은 중립색 말풍선에, AI 답변을 넓은 문서 본문에 배치하고 도구 실행은 접을 수 있는 활동 내역으로 표시한다. 두 프로젝트 모두 화면의 모양과 별도로 세션 식별, 전송 결과, 이전 기록 로드, 읽던 위치 보존을 관리한다. KanVibe에는 이미 다중 공급자 기록 조회 경로가 있으므로 이를 유지하면서 공통 대화 표현을 먼저 적용하고, 실시간 전송에는 별도의 정확한 세션 연결 계약을 사용해야 한다.

분석한 로컬 스냅샷은 Orca `c8a8288a5fd`, Paseo `68818067f`이며, 참조 프로젝트를 실행해 체감 성능을 측정한 결과는 아니다.

## 한눈에 보는 비교

| 영역 | Orca | Paseo | KanVibe 적용 |
| --- | --- | --- | --- |
| 화면 기반 | 데스크톱 React와 모바일 웹 계열 구현, 공유 transcript 계약 | React Native와 웹을 아우르는 공통 컴포넌트, 플랫폼별 viewport 전략 | React와 Flutter의 기술 스택 유지, 같은 시각 규칙을 각각 구현 |
| 사용자 요청 | 오른쪽, 최대 80% 폭, 중립색 표면, 긴 요청 접기 | 오른쪽, surface3 말풍선, 그룹 경계별 간격, 긴 요청 preview | 브랜드 파랑 전체 채움 대신 중립색 요청, AI 본문과 명확히 구분 |
| AI 응답 | 최대 가용 폭의 borderless Markdown, 응답 종료 후 액션 | Markdown 본문, 진행 중 reveal, 긴 내용 렌더 제한 안내 | 가운데 읽기 폭을 제한하고 본문 폭 확보, 원문 복사 보존 |
| 도구·추론 | 묶음 실행 요약, 개별 상세, subagent/background task | tool sequence, thought, turn summary; 모바일 sheet | 단순 role 데이터에서는 도구/추론 접기부터 구현 |
| 이전 기록 | virtualizer + prefetch sentinel + 수동 재시도 | 서버 cursor + 로컬 mounted window + 플랫폼 전략 | 현재 cursor API 유지, 중복 요청·세션 전환 경합 방지 |
| 스크롤 | reader intent와 virtualizer 분리 | sticky-bottom/detached 상태, disclosure hold | 위를 읽는 중 강제 최신 이동 금지, 명시적 최신 이동 제공 |
| 전송 | structured RPC 및 terminal bridge 두 경로 | agentId 기반 daemon, optimistic submission 결과 추적 | 기록 ID를 임의 터미널 입력으로 치환하지 않음 |
| 히스토리 탐색 | 별도 세션 검색 패널과 resume actions | 기존 agent timeline에 history와 live head 결합 | 같은 대화 렌더러로 기록과 채팅을 표현, 탐색 방식만 기기별 조정 |

## Orca 상세

### 화면 진입과 세션 소유권

[orca/src/renderer/src/components/native-chat/NativeChatView.tsx:15](https://github.com/stablyai/orca/blob/c8a8288a5fd022a93d6360c5350579d890b8be18/src/renderer/src/components/native-chat/NativeChatView.tsx#L15)는 `mode === structured`일 때 `NativeChatStructuredSession`을 렌더한다. 이 경로는 세션별 visual/file-link/image owner를 두고 세션 ID가 바뀌면 하위 owner도 바뀐다. 같은 파일 `:64`부터의 bridge 경로는 terminal tab/pane/pty 정보를 `NativeChatSessionGate`에 전달해 실행 대상을 확인한 뒤 `NativeChatResolvedView`를 연다.

따라서 Orca의 chat/terminal 전환은 단순히 문자열을 터미널에 쓰는 UI 스킨이 아니다. 어느 세션·pane에 속한 대화인지 먼저 해결하는 계층이 있다. KanVibe에서 현재 선택한 기록 ID와 현재 터미널이 같다고 추정하면, 과거 대화를 보면서 다른 실행 세션에 메시지를 보낼 수 있다.

### 사용자와 AI 본문의 시각적 계층

`messages/rows/NativeChatUserMessageRow.tsx:95`의 사용자 행은 `items-end`, `:106`의 말풍선은 `max-w-[80%]`, `rounded-xl`, `border-chat-user-border`, `bg-chat-user-surface`다. 사용자 역할은 오른쪽 위치와 표면 차이로 드러나며 밝은 브랜드색을 모든 요청에 채우지 않는다.

`messages/rows/NativeChatAssistantMessageRow.tsx:89`의 AI 행은 `max-w-full`, `select-text`, `text-sm`, `leading-relaxed`의 본문이다. 사용자와 같은 대칭 말풍선을 만들지 않아 긴 설명·목록·코드·표가 읽기 쉽다. `:99`에서 Markdown을 렌더하고 `:112`에서 도구 실행을 이어 붙인다. 복사·시간·fork 액션은 `:113` 이후의 응답 제어부이며 진행 중인 답변의 말단에는 확정된 답변 액션을 숨긴다(`:68–72`).

`appearance/native-chat-appearance-style.ts:17–23`은 채팅 canvas와 transcript column을 별도로 정의한다. 본문은 중앙 `max-width` 열이며 메시지 사이 간격을 유지한다. 큰 데스크톱 화면에서도 한 문장이 화면 전체 폭으로 퍼지지 않는다.

### 긴 입력, 복사와 접근성

`NativeChatUserMessageFold.tsx:27–55`는 메시지 ID에 묶인 펼침 상태와 실제 높이 측정을 함께 사용한다. 내용 길이 추정만으로 접지 않고, 실제로 잘릴 때만 접기 제어를 표시한다. `:65`는 접힌 높이를 제한하지만 전체 원문은 유지한다. `:83–87`은 `aria-expanded`, `aria-controls` 및 펼침 상태를 연결한다.

`NativeChatUserMessageRow.tsx:22–55`의 복사·시간은 hover 가능한 장치에서는 숨겼다가 hover/focus에 드러난다. 터치 환경에서는 hover 조건이 적용되지 않아 기능이 사라지지 않는다. 전송 중 표시는 timestamp 자리를 사용해 행 높이가 흔들리지 않게 한다.

KanVibe 적용 시에는 모바일 복사 버튼을 항상 조작 가능하게 유지하고, 접은 내용도 복사할 때 전체 원문을 사용해야 한다. 화면에 보이는 preview만 복사하면 긴 요청 재사용이 깨진다.

### 도구·추론·에이전트 작업 표현

`tools/runs/NativeChatToolRun.tsx:48–89`는 prose 뒤의 tool blocks, diff, subagent, background task를 하나의 실행 영역으로 받는다. `:104–109`의 disclosure key는 메시지와 실행 상태를 연결하며, 가상화로 행이 잠시 사라져도 펼침 상태를 보존한다. 화면에서 접는 것과 데이터에서 기록을 삭제하는 것은 분리된다.

이 구조는 도구 호출에 입력·출력·상태·연관 ID가 있다는 전제다. KanVibe의 기존 `AggregatedAiMessage`는 role/text/fullText 위주의 평탄한 데이터이므로, 해당 구조만으로 명령 성공 여부나 tool pairing을 추정해 표시하면 안 된다. 현재 계약에서는 tool/reasoning을 접고 system/developer 안내는 기본으로 펼친다. 고급 tool card는 구조화 데이터가 확보된 후 구현해야 한다.

### 타이포그래피와 토큰

`orca/docs/styleguide/native-chat.md`는 본문 14px, secondary 12px, code 12px 등 역할별 크기를 규정하며, root 텍스트 설정에 따라 폰트와 spacing을 함께 조절한다. 핵심은 임의 글자 크기를 화면마다 붙이는 대신 본문·메타·코드의 단계가 같이 움직이게 하는 것이다.

본문·canvas·lifted surface의 색 토큰을 구분하고, transcript full-width row는 clipping되지 않는 inset focus ring을 사용한다. KanVibe는 Orca의 팔레트를 복제하기보다 기존 `--color-*`, `AppColors`, `AppSizes`를 사용해 제품의 브랜드와 테마를 유지하는 편이 맞다.

### 가상화, 이전 기록, 읽던 위치

`viewport/virtualization/use-native-chat-transcript-window.ts:98–112`는 TanStack virtualizer를 사용하며 `anchorTo: end`, `followOnAppend: false`를 설정한다. 이전 기록 추가 시의 위치 보존과 최신 응답 자동 따라가기를 같은 기능으로 취급하지 않는다. `:20–27`에는 overscan 6행과 retired measurement 정리 기준이 있다. 키는 실제 transcript slot identity를 사용한다(`:80–88`).

`viewport/scroll/use-native-chat-transcript-scroll.ts:40–48`은 following과 detached 위치를 별도로 보관한다. `:99–109`에서 독자가 위로 읽기 시작하면 따라가기를 끊는다. 화면 geometry만 보고 다시 최신으로 붙이면 읽던 중 갑자기 이동하므로, virtualizer는 위치를, scroll hook은 독자의 따라가기 의도를 소유한다는 구분이 중요하다.

`viewport/history/use-native-chat-older-history-autoload.ts:6`의 prefetch 여유는 600px다. `:41–49`는 실패하거나 진전 없는 페이지에서 자동 로드를 멈추고 `:56–59`는 수동 재시도로 회복한다. IntersectionObserver가 없는 환경도 수동 로드가 가능하다. 이 숫자를 KanVibe의 요구 성능 측정 없이 그대로 채택할 근거는 없다. 재시도와 무한 반복 방지의 정책을 참고해야 한다.

### 모바일 키보드와 전송 계약

[orca/mobile/src/session/native-chat/mobile-native-chat-lower-geometry.ts:1–31](https://github.com/stablyai/orca/blob/c8a8288a5fd022a93d6360c5350579d890b8be18/mobile/src/session/native-chat/mobile-native-chat-lower-geometry.ts#L1-L31)는 visual viewport와 overflow 조상들의 교집합으로 실제 가용 높이를 계산한다. `:49–65`는 전체 composer나 inset 중 현재 viewport에 들어가는 표면을 고른다. 키보드가 올라왔는데 입력란만 보이고 send/toolbars가 가려지는 문제를 피하려는 구현이다. Flutter에서는 이 JavaScript를 옮기기보다 Scaffold resize, SafeArea, viewInsets 및 스크롤 경계를 활용한다.

`mobile-structured-agent-session-send.ts:20–39`는 `sessionId`, `expectedRuntimeFence`, `clientMessageId`를 필수로 받는다. `:46–50`는 너무 큰 메시지를 전송 전에 거부하고 초안을 유지한다. `:53–70`는 `agentSession.send` RPC에 세션·실행 세대·operation identity를 넣는다. `:71–78`는 accepted/rejected 결과와 확인 cursor를 처리한다. 입력란의 성공처럼 보이는 애니메이션이 실제 전송 성공을 대신하지 않는다.

### 히스토리 탐색은 채팅과 별도 역할

`right-sidebar/ai-vault/session-list/AiVaultSessionHistoryDialog.tsx:29`는 `/resume` 히스토리가 열릴 때 검색 패널을 새로 마운트한다. `:61–79`에서는 dialog 높이 제한, 검색 자동 포커스, 닫힐 때 원래 포커스 복구, Escape로 검색 먼저 지우기를 처리한다. `:85–103`은 workspace 및 agent 필터와 resume 대상을 패널에 제공한다.

`AiVaultSessionDetails.tsx:86–95`는 실제로 resume할 내용이 있는지 검사하고 최초 요청과 최근 세 턴을 요약한다. 즉 Orca 자체도 모든 히스토리 상세가 그대로 live transcript인 것은 아니다. KanVibe 사용자가 요청한 동일 디자인은 Orca의 히스토리 요약 카드를 그대로 복제하는 것보다, KanVibe의 전체 메시지 조회를 공통 transcript 스타일로 표현하는 쪽이 요구에 더 잘 맞는다.

## Paseo 상세

### React Native/웹 공통 화면과 플랫폼별 viewport

[paseo/packages/app/src/agent-stream/view.tsx:32–42](https://github.com/getpaseo/paseo/blob/68818067f2a55c5655dc43f380a143fe4cb7ebb6/packages/app/src/agent-stream/view.tsx#L32-L42)는 UserMessage, AssistantMessage, ToolCall 등을 같은 stream renderer에서 사용한다. `:78–100`은 데이터 모델·presentation·layout·strategy 및 bottom anchor를 구분한다. `agent-stream/layout.ts:31–42`는 history와 liveHead를 각각 받고 두 영역을 같은 layout 결과로 만든다.

KanVibe는 Flutter와 React를 사용하므로 컴포넌트 소스 공유 자체가 목표가 될 수 없다. 같은 role 정책, 메시지 간격, 접기·복사·시간·pagination 동작을 맞추는 것이 재현 가능한 공통 계약이다.

### 요청·응답·도구의 서로 다른 밀도

`components/message.tsx:297–335`의 사용자 행은 오른쪽 정렬, `surface3` 배경, theme radius, 본문 foreground를 사용한다. `UserMessage`의 메시지 ID와 그룹 시작/끝 정보(`:94–110`)가 개별 말풍선과 그룹 간 간격을 결정한다.

`AssistantMessage`는 `:1184–1257`에서 MarkdownRenderer를 사용한다. 진행 중인 문자열 reveal과 완료 본문을 구분하고, 매우 큰 응답은 렌더량 제한 및 안내를 제공한다. 원문을 서버에서 잘라 버리는 것과 렌더 비용 제한은 별개다. 제한이 발생했다면 사용자에게 그 사실을 알려야 한다.

`agent-stream/spacing.ts:29–44`는 사용자 요청끼리는 작은 간격, 요청 다음에는 큰 간격, 도구끼리는 0 간격, 도구와 설명 사이에는 작은 간격을 둔다. 모든 행에 같은 margin을 주는 채팅보다 읽는 단위가 잘 드러난다.

`components/message.tsx:2288–2297`의 도구는 call ID를 기준으로 펼침을 기억하고 모바일에서는 sheet, 큰 화면에서는 inline 상세를 선택한다. `agent-stream/disclosure.tsx:18–38`는 펼침으로 높이가 달라질 때 trigger가 화면에서 유지되도록 `viewport.holdRow`를 호출한다. 접기 UI는 단순 boolean 외에도 scroll 안정성 문제를 가진다.

### 이전 기록: 서버 페이지와 로컬 렌더창을 구분

`hooks/use-load-older-agent-history.ts:41–70`는 중복 로드, client/cursor/hasOlder 부재를 가드한다. cursor는 epoch와 sequence이며 실패 시 toast를 표시하고 finally에서 in-flight 상태를 내린다.

`agent-stream/history-window.ts:4`는 최근 mounted 기본값 20개를 정의한다. `:35–38`은 최근 20개 경계에서 턴이 끊어지면 이전으로 확장한다. `use-stream-history-window.ts:108–110`은 이미 로드되어 있지만 렌더하지 않은 내용을 먼저 펼치고, 그것이 없을 때 원격 페이지를 읽는다. 서버 pagination과 화면 virtualization은 서로 다른 비용을 줄인다.

`web-virtualization.ts:9`는 웹 부분 가상화 기준 100개를, `:51–77`은 종류별 높이 추정치를 정의한다. 글만 있는 요청과 이미지 요청, 긴 Markdown, 접힌 도구 행의 높이가 다르다는 전제다. KanVibe는 먼저 기존 40개 메시지 cursor를 활용하고, 실제 긴 대화 성능 근거가 쌓이면 가상화 범위를 정하는 것이 과도한 의존성 추가를 줄인다.

### 최신 따라가기와 상태 렌더 비용

`bottom-anchor-controller.ts:4`는 sticky-bottom/detached 두 모드를 정의한다. `:6–21`은 첫 진입·resume·전송·최신 이동 요청을 구분하고 history/viewport/content가 준비되지 않은 상태를 명시한다. 키보드나 화면 폭 변경도 따라가기와 측정의 재검증 대상이다.

`near-bottom.ts:3–6`은 최신 여부를 viewport가 쓰고 최신 이동 버튼만 구독하게 한다. 스크롤할 때 transcript 전체가 다시 렌더되지 않게 책임을 분리한 것이다. 이는 대용량 메시지에서 의미가 있으며, KanVibe의 작은 목록에 처음부터 외부 store를 추가하라는 뜻은 아니다.

### Markdown, 모바일 입력과 전송 결과

`components/markdown/renderer.tsx:57–73`은 모든 Markdown 표면이 같은 엔진을 사용하며 streaming일 때 완료된 block과 현재 tail을 분리한다. 매 프레임 전체 AST를 다시 분석하지 않도록 completed lines에 대해 boundary를 memo한다. 내용 선택, 링크 열기, 이미지 경로 처리도 공통 renderer로 들어간다.

`composer/input/input.tsx:79–89`는 iOS 하드웨어 키보드 제출, IME composition, form factor, 높이 계산을 따로 관리한다. 한국어 조합 확정 Enter를 메시지 전송으로 오인하지 않는지가 KanVibe에서도 필수 확인 항목이다.

`composer/dock/index.native.tsx:37–95`는 header 아래의 고정 viewport를 먼저 측정하고 safe-area/keyboard shift로 전체 composer capacity를 계산한다. 프레임마다 layout을 변경하기보다 transform 계층으로 움직임을 처리한다.

`composer/submission/writer.ts:48–63`는 서버가 canonicalSubmittedPrompts를 지원하면 begin/accept/reject를 세션 store에 기록한다. 지원하지 않는 서버도 optimistic 항목을 reject 시 제거한다(`:37–44`). 확인되지 않은 전송을 영구 사용자 메시지로 남기지 않는다.

## KanVibe의 기존 기반과 변경 경계

현재 데스크톱은 `src/desktop/renderer/routes/TaskDetailRoute.tsx`의 InlineAiChatView에서 공급자·역할 필터, 검색, 세션 목록, 이전 메시지 cursor, 전문 복사를 이미 제공한다. 기존 UI의 넓은 provider rail, 세션 카드, 양쪽 대칭의 강한 말풍선은 작업 기록을 읽는 데 필요한 본문 폭을 줄인다. 목록을 중립색 행으로 낮추고 AI 답변을 문서로 펼치면 데이터 경로를 바꾸지 않고도 개선할 수 있다.

`src/desktop/renderer/components/AiSessionMessageContent.tsx:78–85`는 marked 결과를 DOMPurify로 정제한다. `:65–72`는 외부 링크의 noopener/noreferrer와 이미지의 referrer 정책을 추가한다. 기존 보안·Markdown·Mermaid 처리를 폐기하고 새 renderer를 중복 도입할 이유는 없다.

모바일 `mobile/lib/src/features/chat_history/domain/chat_history.dart:1–35`는 세션 ID/provider/title/updatedAt과 role/text/timestamp를 읽는다. `fullText`를 우선하므로 복사 원문 기반이 이미 있다. 다만 실행 pane ID나 전송 capability는 포함하지 않는다. 기존 Riverpod controller는 연결된 PC와 조회 generation을 기준으로 요청 결과를 처리하므로 UI 전용 상태와 네트워크 상태의 책임을 유지해야 한다.

현재 기록 계약에는 구조화된 tool-call ID, 호출 상태, 첨부, turn ID가 없다. 그러므로 이번 UI에서 확실히 표현할 수 있는 것은 요청·답변·역할별 접기·시간·원문이며, 도구 성공 배지·에이전트 진행률·세션 resume 가능 여부를 임의로 만들어서는 안 된다.

## 구현에 바로 적용할 결정

| 결정 | 이유 | 확인할 사용자 행동 |
| --- | --- | --- |
| 사용자 중립색 말풍선, AI 넓은 본문 | 두 참조의 핵심 공통 문법이며 긴 기술 답변에 유리 | 긴 코드/링크/한글에서 가로 overflow 없음 |
| 데스크톱 master-detail, 폰 list→detail, 태블릿 분할 | 데이터는 같아도 가용 폭과 입력 방식이 다름 | 뒤로 갔을 때 목록과 검색 유지, 좁은 창에서 본문 확보 |
| 도구/추론은 접어서 요약하고 전문 접근 제공 | 활동 로그가 최종 답변을 압도하지 않게 함 | 펼침/접기와 복사가 전체 내용을 유지 |
| 이전 기록 추가 시 읽던 위치 유지 | 도구 펼침과 prepend는 모두 행 높이를 바꿈 | 이전 페이지 로드 뒤 같은 문단을 계속 읽음 |
| 명시적 최신 이동 | 독자의 위쪽 읽기 의도를 보존 | 위로 읽는 중 새 데이터가 강제 점프하지 않음 |
| loading/empty/error/retry를 해당 pane에 유지 | 네트워크 실패와 기록 없음은 의미가 다름 | 실패 후 필터·검색을 잃지 않고 재시도 |
| 정확한 세션 전송 계약을 확인한 경우에 composer 활성화 | 기록 ID와 raw terminal input은 목적지가 같다는 보장이 없음 | 전송 결과 확인, 오류 시 초안 유지, 중복 제출 방지 |

## 검증에 남겨야 할 증거

이 문서는 참조 프로젝트의 소스 분석 결과다. KanVibe 구현 완료 및 성능 검증을 대신하지 않는다. 구현 검증에서는 데스크톱 실제 Electron과 모바일 실제 기기/시뮬레이터에서 목록·상세·검색·필터·이전 기록·복사·재시도·좁은 폭·큰 글자·키보드 상태를 확인하고 화면 증거를 남긴다. 실시간 전송을 구현하면 전송 API의 세션 격리와 결과 확인을 별도로 검증한다.

## KanVibe 구현 결과

데스크톱은 기존 기록 API와 안전한 Markdown/Mermaid 렌더러를 유지하면서 목록과 transcript를 정리했다. 720px보다 좁은 채팅 영역에서는 목록과 본문 중 하나를 표시하고, 뒤로 돌아가도 검색·provider 필터를 유지한다. 이전 페이지를 추가할 때 스크롤 높이 차이를 보정하고, 동일한 도구 메시지가 반복돼도 펼친 행의 식별자를 유지한다. Mermaid는 정제 과정에서 글자가 사라지지 않도록 HTML label 대신 SVG text를 출력한다.

모바일은 같은 메시지 표현을 ChatConversationView와 ChatMessageTile로 분리했다. 768dp 이상에서는 세션 목록과 대화를 나란히 표시하고 휴대폰에서는 기존 Navigator의 목록→대화 흐름을 유지한다. AI 본문은 선택 가능한 Markdown, 사용자 입력은 선택 가능한 원문으로 표시한다. 전문 복사·시간·접기·최신 이동·재시도를 제공한다. Markdown은 flutter_markdown_plus 1.0.12의 MarkdownBody를 사용한다. [공식 MarkdownBody API](https://pub.dev/documentation/flutter_markdown_plus/latest/flutter_markdown_plus/MarkdownBody-class.html)는 이를 “A non-scrolling widget that parses and displays Markdown.”로 정의한다.

사용자가 요구한 채팅 모드는 현재 포커스된 pane의 CLI를 제어하는 화면이다. 데스크톱 채팅 dock과 모바일 pane의 채팅 전환은 같은 실행 프로세스에 연결하고, 히스토리는 별도로 연다. 현재 실행 중인 공급자는 프로세스에서 감지한다. 프롬프트는 bracketed paste 후 별도 Enter로 전달하며, 중단과 CLI 선택기 조작을 제공한다.

Orca의 terminal bridge처럼 입력 대상과 대화 파일의 소유권을 구분한다. task·pane·공급자 PID·시작 시각을 묶어 전송 직전에 확인한다. Claude/Codex hook의 세션 ID를 실제 조상 프로세스와 연결하고, 정확한 열린 대화 파일을 확인할 수 있는 경우에도 그 파일만 읽는다. 최신 히스토리 순서로 실행 대상을 추정하지 않는다. 대화 연결 전에는 현재 CLI 화면을 표시하고, 파일이 연결되면 같은 메시지 렌더러로 응답을 갱신한다.

모델 변경은 CLI의 실제 `/model` 선택기를 연다. 특히 [Orca의 Claude/Codex 옵션 계약](https://github.com/stablyai/orca/blob/c8a8288a5fd022a93d6360c5350579d890b8be18/src/shared/agent-session/options/agent-session-option-catalog-claude-codex.ts)은 Claude의 모델 명령과 Codex의 대화형 선택기를 구분한다. KanVibe는 화면에 나온 목록을 방향·확인·취소 버튼으로 조작하며, 명령을 썼다는 이유만으로 변경 성공을 표시하지 않는다. 터미널에 기존 초안이 있거나 입력 위치를 확인할 수 없으면 프롬프트와 모델 명령을 막고 CLI 조작 및 터미널 전환을 제공한다.

현재 새 프롬프트·모델 선택은 일반 터미널·tmux·Herdr의 Claude/Codex를 대상으로 한다. Herdr는 해당 task의 실제 클라이언트가 포커스한 pane을 읽고, native 화면 스트림을 headless 터미널로 복원해 입력 위치를 확인한다. Zellij는 설치된 CLI가 정확한 OS 프로세스 식별자를 제공하지 않아 전송을 차단한다. 원격 일반 터미널의 SSH 뒤 프로세스나 원격 데스크톱 tmux 클라이언트도 로컬 PID로 추정하지 않는다. 해당 원격 경로까지 지원 완료했다는 의미는 아니다. 실제 모델 API 응답 품질과 연결 비용은 별도 검증 대상이다.
