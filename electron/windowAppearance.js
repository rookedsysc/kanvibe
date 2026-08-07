/** 터미널 배경이 완전히 불투명한 상태. src/lib/terminalOpacity.ts의 OPAQUE_TERMINAL_OPACITY와 같은 값이다 */
const OPAQUE_TERMINAL_OPACITY = 1;

const OPAQUE_WINDOW_BACKGROUND_COLOR = "#ffffff";
/**
 * Electron의 backgroundColor는 alpha가 앞에 오는 #AARRGGBB 순서다. alpha를 정확히 0으로 주면
 * 창 서버가 창을 제대로 합성하지 못하고 불투명하게 남는 경우가 있어, 8비트 alpha가 표현할 수 있는
 * 가장 작은 0이 아닌 값(0x01)을 쓴다.
 *
 * RGB는 터미널 배경과 같은 어두운 색으로 둔다. Ghostty는 같은 자리에 흰색을 쓰지만 그 근거는
 * "Terminal.app의 모습에 가깝다"는 밝은 테마 기준이라 다크 앱인 KanVibe에는 옮겨올 수 없다.
 * 창을 투명하게 합성하지 못하는 환경에서는 이 RGB가 그대로 드러나므로, 어두운 색이어야 평소의
 * 불투명 터미널처럼 degrade한다.
 */
const TRANSPARENT_WINDOW_BACKGROUND_COLOR = "#010a0a0a";

/** 터미널 뒤가 비쳐 보여야 하는 설정인지 판단한다. 설정을 읽지 못했으면 불투명으로 본다 */
function isTransparentTerminal(terminalOpacity) {
  return Number.isFinite(terminalOpacity) && terminalOpacity < OPAQUE_TERMINAL_OPACITY;
}

/**
 * 터미널 투명도 설정에 맞는 BrowserWindow 배경 옵션을 만든다.
 * transparent는 창 생성 시점에만 정할 수 있으므로 저장된 설정을 창을 만들기 전에 읽어 넘겨야 한다.
 * 투명도를 쓰지 않는 사용자는 창 그림자와 모서리 처리가 달라지지 않도록 기존 불투명 배경을 그대로 유지한다.
 */
function createWindowBackgroundOptions(terminalOpacity) {
  if (!isTransparentTerminal(terminalOpacity)) {
    return { backgroundColor: OPAQUE_WINDOW_BACKGROUND_COLOR };
  }

  return {
    transparent: true,
    backgroundColor: TRANSPARENT_WINDOW_BACKGROUND_COLOR,
  };
}

/**
 * Linux에서 투명 창은 X11/Wayland 표면이 alpha 채널을 가진 ARGB visual일 때만 합성된다.
 * Chromium은 이 visual을 enable-transparent-visuals 스위치가 있을 때만 고르므로,
 * 터미널을 반투명하게 쓸 때 이 스위치를 켠다. macOS는 창 서버가 알아서 처리해 필요 없다.
 */
function shouldEnableTransparentVisuals(platform, terminalOpacity) {
  return platform === "linux" && isTransparentTerminal(terminalOpacity);
}

module.exports = {
  OPAQUE_TERMINAL_OPACITY,
  createWindowBackgroundOptions,
  shouldEnableTransparentVisuals,
};
