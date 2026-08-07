/** 터미널 배경이 완전히 불투명한 상태. 이 값이면 창과 페이지 배경을 그대로 둔다 */
export const OPAQUE_TERMINAL_OPACITY = 1;

/**
 * 완전한 0을 피하기 위한 최소 불투명도. alpha가 정확히 0이면 창 서버가 창을 합성하지 않고
 * 불투명하게 남기는 플랫폼이 있어, Ghostty가 backgroundOpacity를 0.001...1로 클램프하는 것을
 * 그대로 따른다(TerminalWindow.swift).
 */
const MIN_TERMINAL_OPACITY = 0.001;

/** 터미널 불투명도를 허용 범위로 보정한다. 숫자가 아니면 불투명으로 되돌린다 */
export function clampTerminalOpacity(opacity: number): number {
  if (!Number.isFinite(opacity)) {
    return OPAQUE_TERMINAL_OPACITY;
  }

  return Math.min(Math.max(opacity, MIN_TERMINAL_OPACITY), OPAQUE_TERMINAL_OPACITY);
}

/** 터미널 뒤가 비쳐 보여야 하는 설정인지 판단한다 */
export function isTerminalTransparent(opacity: number): boolean {
  return clampTerminalOpacity(opacity) < OPAQUE_TERMINAL_OPACITY;
}
