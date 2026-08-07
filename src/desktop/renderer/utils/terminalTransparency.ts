import { isTerminalTransparent } from "@/lib/terminalOpacity";

/**
 * 터미널 뒤가 비쳐 보이도록 페이지 배경을 걷어내는 문서 속성을 반영한다.
 * 실제 반투명 배경은 xterm 테마가 그리므로 여기서는 터미널을 덮는 배경만 투명하게 만든다.
 */
export function applyTerminalTransparency(terminalOpacity: number) {
  const root = document.documentElement;

  if (isTerminalTransparent(terminalOpacity)) {
    root.dataset.terminalTransparent = "true";
    return;
  }

  delete root.dataset.terminalTransparent;
}
