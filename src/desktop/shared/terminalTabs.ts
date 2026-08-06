/**
 * 터미널 탭 하나를 나타낸다.
 * tmux window, zellij tab, KanVibe가 직접 소유하는 plain terminal 탭이 모두 이 모양으로 표현된다.
 */
export interface TerminalTab {
  /** 멀티플렉서가 부여한 안정 식별자(tmux window_id, zellij tab id) 또는 KanVibe가 생성한 탭 식별자 */
  id: string;
  /** 탭 바에 표시할 순서. 0부터 시작한다 */
  index: number;
  name: string;
  isActive: boolean;
}

export interface TerminalTabListResult {
  ok: boolean;
  tabs: TerminalTab[];
  error?: string;
}

/**
 * Electron main이 `before-input-event`에서 가로챈 탭 단축키를 렌더러로 넘길 때 쓰는 명령.
 * 터미널이 입력을 먼저 소비하지 않도록 키 판정은 main에서 끝내고 의미만 전달한다.
 */
export type TerminalTabShortcutCommand =
  | { type: "new-tab" }
  | { type: "close-tab" }
  | { type: "close-window" }
  | { type: "previous-tab" }
  | { type: "next-tab" }
  | { type: "go-to-tab"; position: number };

export interface TerminalTabMutationResult {
  ok: boolean;
  /** 탭을 닫은 뒤 남은 탭 수. 0이면 호출자가 윈도우를 닫는다 */
  remainingCount?: number;
  error?: string;
}
