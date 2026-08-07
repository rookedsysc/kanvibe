/**
 * 터미널 탭 하나를 나타낸다.
 * tmux window, zellij tab, KanVibe가 직접 소유하는 plain terminal 탭이 모두 이 모양으로 표현된다.
 */
export interface TerminalTab {
  /** 멀티플렉서가 부여한 안정 식별자(tmux window_id, zellij tab id) 또는 KanVibe가 생성한 탭 식별자 */
  id: string;
  /**
   * 멀티플렉서가 매긴 원본 인덱스. 좌표계가 세션 타입마다 달라 표시 순서로 쓰면 안 된다.
   * tmux는 `window_index`라 `base-index 1` 설정에서 1부터 시작하고 window를 닫으면 번호에 구멍이 생긴다.
   * zellij는 `position`, terminal 세션은 배열 위치라 둘 다 0부터 빈틈없이 이어진다.
   * 표시 순서와 `Mod+{n}` 번호는 이 값이 아니라 배열 위치를 쓴다.
   */
  nativeIndex: number;
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
