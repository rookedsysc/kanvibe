/** 터미널 배경이 완전히 불투명한 상태. src/lib/terminalOpacity.ts의 OPAQUE_TERMINAL_OPACITY와 같은 값이다 */
const OPAQUE_TERMINAL_OPACITY = 1;

const OPAQUE_WINDOW_BACKGROUND_COLOR = "#ffffff";
const TRANSPARENT_WINDOW_BACKGROUND_COLOR = "#00000000";

/**
 * 터미널 투명도 설정에 맞는 BrowserWindow 배경 옵션을 만든다.
 * transparent는 창 생성 시점에만 정할 수 있으므로 저장된 설정을 창을 만들기 전에 읽어 넘겨야 한다.
 * 투명도를 쓰지 않는 사용자는 창 그림자와 모서리 처리가 달라지지 않도록 기존 불투명 배경을 그대로 유지한다.
 */
function createWindowBackgroundOptions(terminalOpacity) {
  const isTransparentTerminal = Number.isFinite(terminalOpacity) && terminalOpacity < OPAQUE_TERMINAL_OPACITY;

  if (!isTransparentTerminal) {
    return { backgroundColor: OPAQUE_WINDOW_BACKGROUND_COLOR };
  }

  return {
    transparent: true,
    backgroundColor: TRANSPARENT_WINDOW_BACKGROUND_COLOR,
  };
}

module.exports = { OPAQUE_TERMINAL_OPACITY, createWindowBackgroundOptions };
