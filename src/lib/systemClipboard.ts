/** Electron 메인 빌드에서도 사용할 수 있게 렌더러 전역 선언 대신 필요한 브리지 기능만 정의한다. */
interface DesktopClipboardBridge {
  writeSystemClipboard?: (text: string) => Promise<void>;
}

/** 데스크톱에서는 문서 포커스·브라우저 권한에 의존하지 않는 메인 프로세스로 복사하고, 웹에서는 브라우저 API를 사용한다. 실패는 호출자에게 전달한다. */
export async function writeSystemClipboard(text: string): Promise<void> {
  const { kanvibeDesktop } = globalThis as { kanvibeDesktop?: DesktopClipboardBridge };
  if (kanvibeDesktop?.writeSystemClipboard) {
    await kanvibeDesktop.writeSystemClipboard(text);
    return;
  }
  await navigator.clipboard.writeText(text);
}
