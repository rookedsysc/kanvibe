/** 터미널 배경이 완전히 불투명한 상태. src/lib/terminalOpacity.ts의 OPAQUE_TERMINAL_OPACITY와 같은 값이다 */
const OPAQUE_TERMINAL_OPACITY = 1;

const OPAQUE_WINDOW_BACKGROUND_COLOR = "#ffffff";
/**
 * alpha를 정확히 0(#00000000)으로 주면 macOS 창 서버가 창을 제대로 합성하지 못하고
 * 불투명 검정으로 남는 경우가 있다. Ghostty도 같은 이유로 배경색에 완전한 clear 대신
 * 0에 가까운 alpha를 쓴다(TerminalWindow.swift의 backgroundColor = .white.withAlphaComponent(0.001)).
 * 8비트 alpha가 표현할 수 있는 가장 작은 0이 아닌 값(1/255)을 같은 목적으로 쓴다.
 */
const TRANSPARENT_WINDOW_BACKGROUND_COLOR = "#00000001";

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

/**
 * Apple Silicon macOS에서 Electron의 GPU 컴포지터가 transparent 창의 웹 콘텐츠를
 * 불투명하게 그려버리는 경우가 있다(Chromium GPU 프로세스가 알파 채널을 창 표면까지
 * 전달하지 못함). 터미널을 반투명하게 쓸 때만 GPU 가속을 꺼서 이 문제를 피하고,
 * 투명도를 쓰지 않는 사용자는 기존 GPU 가속 렌더링을 그대로 유지한다.
 */
function shouldDisableGpuAccelerationForTransparency(terminalOpacity) {
  return Number.isFinite(terminalOpacity) && terminalOpacity < OPAQUE_TERMINAL_OPACITY;
}

module.exports = {
  OPAQUE_TERMINAL_OPACITY,
  createWindowBackgroundOptions,
  shouldDisableGpuAccelerationForTransparency,
};
