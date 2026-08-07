/** 터미널 배경이 완전히 불투명한 상태. 이 값이면 창과 페이지 배경을 그대로 둔다 */
export const OPAQUE_TERMINAL_OPACITY = 1;

/** 터미널 글자가 뒤 배경에 묻히지 않는 최소 불투명도 */
export const MIN_TERMINAL_OPACITY = 0.3;

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
