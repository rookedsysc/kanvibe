import type { IDisposable, Terminal } from "@xterm/xterm";

/** 터미널이 시스템 클립보드에 값을 넣을 때 쓰는 OSC 시퀀스 번호 */
const OSC_CLIPBOARD_IDENTIFIER = 52;

/**
 * 클립보드에 넣을 base64 페이로드 상한.
 * tmux나 zellij 너머의 임의 프로세스가 보내는 값이므로 렌더러가 무한정 받아들이지 않는다.
 */
const MAX_CLIPBOARD_PAYLOAD_LENGTH = 1_000_000;

/** OSC 52의 읽기 요청 표기. 터미널 내용이 원격으로 새는 것을 막기 위해 응답하지 않는다 */
const CLIPBOARD_READ_REQUEST = "?";

/**
 * xterm.js가 OSC 52를 처리하도록 핸들러를 등록한다.
 * xterm.js는 OSC 52를 기본 처리하지 않아, 등록하지 않으면 tmux와 zellij가 보낸 복사 요청이 그대로 버려진다.
 */
export function installOsc52ClipboardHandler(terminal: Terminal): IDisposable {
  return terminal.parser.registerOscHandler(OSC_CLIPBOARD_IDENTIFIER, (data) => {
    const clipboardText = readOsc52ClipboardText(data);
    if (clipboardText !== null) {
      void writeSystemClipboard(clipboardText);
    }

    /** 읽기 요청이나 잘못된 페이로드도 소비해, 처리되지 않은 시퀀스가 화면에 출력되지 않게 한다 */
    return true;
  });
}

/**
 * OSC 52 페이로드에서 클립보드에 넣을 문자열을 뽑는다.
 * @param data `<선택 영역>;<base64 본문>` 형식의 OSC 52 본문
 * @returns 클립보드에 기록할 문자열. 읽기 요청이거나 해석할 수 없으면 null
 */
export function readOsc52ClipboardText(data: string): string | null {
  const separatorIndex = data.indexOf(";");
  if (separatorIndex === -1) {
    return null;
  }

  const encodedText = data.slice(separatorIndex + 1);
  if (!encodedText || encodedText === CLIPBOARD_READ_REQUEST) {
    return null;
  }

  if (encodedText.length > MAX_CLIPBOARD_PAYLOAD_LENGTH) {
    return null;
  }

  return decodeBase64Utf8(encodedText);
}

/** base64 본문을 UTF-8 문자열로 되돌린다. 한글처럼 멀티바이트 문자가 깨지지 않도록 바이트 단위로 디코딩한다 */
function decodeBase64Utf8(encodedText: string): string | null {
  try {
    const binaryText = atob(encodedText);
    const bytes = Uint8Array.from(binaryText, (character) => character.charCodeAt(0));
    return new TextDecoder().decode(bytes);
  } catch {
    return null;
  }
}

/**
 * 데스크톱 렌더러에만 주입되는 클립보드 브리지.
 * 이 모듈은 `src/lib` 아래라 Electron 메인 프로세스 빌드에도 함께 컴파일되는데, 그 빌드는 렌더러 전역 선언을
 * 포함하지 않는다. 그래서 렌더러의 `Window` 타입에 기대지 않고 실제로 쓰는 기능만 좁게 선언한다.
 */
interface DesktopClipboardBridge {
  writeSystemClipboard?: (clipboardText: string) => Promise<void>;
}

/**
 * 복사 요청을 시스템 클립보드에 반영한다.
 * 데스크톱에서는 Electron 메인 프로세스를 거친다. 렌더러의 `navigator.clipboard`는 문서 포커스를 요구하고,
 * 포커스가 있어도 성공을 반환한 채 클립보드를 그대로 두는 경우가 있어 터미널 출력이 촉발하는 복사를 놓친다.
 * 데스크톱 브리지가 없는 웹 터미널에서는 렌더러 클립보드 API가 유일한 경로다.
 */
async function writeSystemClipboard(clipboardText: string): Promise<void> {
  try {
    const { kanvibeDesktop } = globalThis as { kanvibeDesktop?: DesktopClipboardBridge };
    const writeDesktopClipboard = kanvibeDesktop?.writeSystemClipboard;
    if (writeDesktopClipboard) {
      await writeDesktopClipboard(clipboardText);
      return;
    }

    await navigator.clipboard.writeText(clipboardText);
  } catch (error) {
    console.warn("터미널 클립보드 복사 실패:", error);
  }
}
