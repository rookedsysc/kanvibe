import type { MouseEvent } from "react";

function findTextField(target: EventTarget): HTMLInputElement | HTMLTextAreaElement | null {
  if (!(target instanceof Element)) return null;
  const field = target.closest<HTMLInputElement | HTMLTextAreaElement>("textarea, input");
  // 체크박스나 파일 선택처럼 글자를 받지 않는 입력은 selectionStart가 null이다.
  return field && field.selectionStart !== null ? field : null;
}

/** 우클릭한 자리에 맞는 OS 글자 메뉴를 연다. 데스크톱 브리지가 없는 환경에서는 브라우저 기본 동작에 맡긴다 */
export function requestTextContextMenu(event: MouseEvent<HTMLElement>) {
  const showTextContextMenu = window.kanvibeDesktop?.showTextContextMenu;
  if (!showTextContextMenu) return;
  event.preventDefault();
  const textField = findTextField(event.target);
  const hasSelection = textField
    ? textField.selectionStart !== textField.selectionEnd
    : Boolean(window.getSelection()?.toString());
  showTextContextMenu({ isEditable: textField !== null, hasSelection });
}
