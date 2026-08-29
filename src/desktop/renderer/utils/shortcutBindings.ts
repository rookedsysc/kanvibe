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

/**
 * 녹화 화면이 열려 있는지. 녹화 중에 눌린 조합은 명령이 아니라 저장할 값이다.
 *
 * 녹화 화면은 입력 요소에 포커스를 두지 않아 keydown 대상이 대개 `body`다.
 * 그래서 `isShortcutCaptureTarget`만으로는 창 닫기 같은 전역 처리기를 막지 못한다.
 */
let shortcutCaptureActive = false;

export function isShortcutCaptureActive(): boolean {
  return shortcutCaptureActive;
}

/** 녹화 시작/종료를 렌더러 전역과 Electron main 양쪽에 알린다 */
export function setShortcutCaptureActive(isActive: boolean): void {
  shortcutCaptureActive = isActive;
  window.kanvibeDesktop?.notifyShortcutCaptureChanged?.(isActive);
}

/**
 * 캐시가 저장된 표를 실제로 담고 있는지.
 *
 * 조회에 실패한 창의 캐시는 기본값이다. 저장은 표를 통째로 치환하므로, 그 캐시를 그대로 저장하면
 * 저장돼 있던 다른 명령의 재배정이 함께 지워진다. 그래서 저장 전에 이 값을 먼저 본다.
 */
let shortcutBindingsLoaded = false;

export function hasLoadedShortcutBindings(): boolean {
  return shortcutBindingsLoaded;
}

/**
 * 저장된 재배정을 캐시에 채운다. 실패해도 기본 단축키로는 계속 동작해야 한다.
 *
 * 다시 읽기에 실패하면 캐시는 다른 창이 바꾼 값을 놓친 옛 표다. 그 표로 저장해도 남의 재배정을
 * 지우므로, 성공한 조회가 있었더라도 실패한 순간부터는 담고 있지 않다고 본다.
 */
export async function loadShortcutBindings(): Promise<boolean> {
  try {
    publishShortcutBindings(await getShortcutBindings());
    shortcutBindingsLoaded = true;
  } catch (error) {
    console.error("단축키 설정 조회 실패:", error);
    shortcutBindingsLoaded = false;
  }

  return shortcutBindingsLoaded;
}

/** 단축키 표를 저장하고, 렌더러 캐시와 Electron main 캐시를 함께 갱신한다 */
export async function saveShortcutBindings(bindings: ShortcutBindings): Promise<void> {
  await setShortcutBindings(bindings);
  publishShortcutBindings(bindings);
  window.kanvibeDesktop?.notifyShortcutBindingsChanged?.();
}
