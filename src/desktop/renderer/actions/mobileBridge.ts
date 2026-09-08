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

export function listPairedMobileDevices(): Promise<PairedMobileDevice[]> {
  return invokeDesktop("mobileBridge", "listPairedMobileDevices");
}

export function unpairMobileDevice(deviceId: string): Promise<void> {
  return invokeDesktop("mobileBridge", "unpairMobileDevice", deviceId);
}
