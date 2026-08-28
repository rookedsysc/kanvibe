import { useSyncExternalStore } from "react";
import { getShortcutBindings, setShortcutBindings } from "@/desktop/renderer/actions/appSettings";
import { DEFAULT_SHORTCUT_BINDINGS, type ShortcutBindings } from "@/desktop/shared/shortcutBindings";

/**
 * 키 입력 처리는 동기라 저장소를 그때 읽어올 수 없다.
 * 그래서 앱이 뜰 때 한 번 읽어 여기에 담아 두고, 설정 화면이 바꿀 때마다 다시 채운다.
 */
let currentShortcutBindings: ShortcutBindings = DEFAULT_SHORTCUT_BINDINGS;
const bindingListeners = new Set<() => void>();

export function readShortcutBindings(): ShortcutBindings {
  return currentShortcutBindings;
}

function subscribeToShortcutBindings(listener: () => void): () => void {
  bindingListeners.add(listener);
  return () => {
    bindingListeners.delete(listener);
  };
}

function publishShortcutBindings(bindings: ShortcutBindings): void {
  currentShortcutBindings = bindings;
  for (const listener of bindingListeners) {
    listener();
  }
}

export function useShortcutBindings(): ShortcutBindings {
  return useSyncExternalStore(subscribeToShortcutBindings, readShortcutBindings, readShortcutBindings);
}

/**
 * 단축키를 녹화 중인 UI 안에서 난 입력인지 본다.
 * 녹화 중에는 누른 조합이 명령으로 실행되면 안 되고 새 단축키 값으로만 읽혀야 한다.
 */
export function isShortcutCaptureTarget(eventTarget: EventTarget | null): boolean {
  return eventTarget instanceof Element && eventTarget.closest('[data-shortcut-capture="true"]') !== null;
}

/** 저장된 재배정을 캐시에 채운다. 실패해도 기본 단축키로는 계속 동작해야 한다 */
export async function loadShortcutBindings(): Promise<void> {
  try {
    publishShortcutBindings(await getShortcutBindings());
  } catch (error) {
    console.error("단축키 설정 조회 실패:", error);
  }
}

/** 단축키 표를 저장하고, 렌더러 캐시와 Electron main 캐시를 함께 갱신한다 */
export async function saveShortcutBindings(bindings: ShortcutBindings): Promise<void> {
  await setShortcutBindings(bindings);
  publishShortcutBindings(bindings);
  window.kanvibeDesktop?.notifyShortcutBindingsChanged?.();
}
