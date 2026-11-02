import { invokeDesktop } from "@/desktop/renderer/ipc";

/**
 * 설정 화면이 모바일 연결을 다루는 통로.
 *
 * pane 스트림은 여기 없다. 그쪽은 hook 서버의 `/api/mobile/*`가 직접 부르고 화면이 관여하지 않는다.
 */

export interface PairedMobileDevice {
  deviceId: string;
  deviceName: string;
  pairedAt: string;
}

/** 화면에 띄울 6자리 코드를 발급한다 */
export function startMobilePairing(): Promise<{ code: string; expiresAt: number }> {
  return invokeDesktop("mobileBridge", "startMobilePairing");
}

/** 페어링 화면을 닫는다. 코드도 함께 죽는다 */
export function stopMobilePairing(): Promise<void> {
  return invokeDesktop("mobileBridge", "stopMobilePairing");
}

/**
 * 데스크탑이 아직 들고 있는 코드. 없으면 null.
 *
 * 코드는 만료 말고도 사라진다. 실패가 상한에 닿으면 데스크탑이 코드를 버리는데, 화면은 그것을 알 길이 없어
 * 남은 시간이 흐르는 6자리를 계속 보여 준다. 사용자는 그 코드로 무엇도 연결되지 않는 이유를 찾지 못한다.
 */
export function readMobilePairingCode(): Promise<string | null> {
  return invokeDesktop("mobileBridge", "readMobilePairingCode");
}

export function listPairedMobileDevices(): Promise<PairedMobileDevice[]> {
  return invokeDesktop("mobileBridge", "listPairedMobileDevices");
}

export function unpairMobileDevice(deviceId: string): Promise<void> {
  return invokeDesktop("mobileBridge", "unpairMobileDevice", deviceId);
}
