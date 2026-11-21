import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { requestTextContextMenu } from "../textContextMenu";

function Surface() {
  return (
    <div onContextMenu={requestTextContextMenu}>
      <p>선택할 글자</p>
      <textarea aria-label="draft" defaultValue="입력한 글자" />
      <input type="checkbox" aria-label="option" />
    </div>
  );
}

function openWithBridge() {
  const showTextContextMenu = vi.fn();
  vi.stubGlobal("kanvibeDesktop", { showTextContextMenu });
  render(<Surface />);
  return showTextContextMenu;
}

function selectAllOf(node: Node) {
  const range = document.createRange();
  range.selectNodeContents(node);
  const selection = window.getSelection()!;
  selection.removeAllRanges();
  selection.addRange(range);
}

afterEach(() => {
  cleanup();
  window.getSelection()?.removeAllRanges();
  vi.unstubAllGlobals();
});

describe("requestTextContextMenu", () => {
  it("선택한 글자 위에서 우클릭하면 기본 동작을 막고 복사할 수 있는 메뉴를 요청한다", () => {
    const showTextContextMenu = openWithBridge();
    const paragraph = screen.getByText("선택할 글자");
    selectAllOf(paragraph);

    const wasDefaultAllowed = fireEvent.contextMenu(paragraph);

    expect(wasDefaultAllowed).toBe(false);
    expect(showTextContextMenu).toHaveBeenCalledExactlyOnceWith({ isEditable: false, hasSelection: true });
  });

  it("선택한 글자가 없으면 복사할 것이 없다고 알린다", () => {
    const showTextContextMenu = openWithBridge();

    fireEvent.contextMenu(screen.getByText("선택할 글자"));

    expect(showTextContextMenu).toHaveBeenCalledExactlyOnceWith({ isEditable: false, hasSelection: false });
  });

  it("입력창에서 범위를 선택하고 우클릭하면 잘라내기와 붙여넣기까지 쓸 수 있는 메뉴를 요청한다", () => {
    const showTextContextMenu = openWithBridge();
    const draft = screen.getByLabelText<HTMLTextAreaElement>("draft");
    draft.setSelectionRange(0, 3);

    fireEvent.contextMenu(draft);

    expect(showTextContextMenu).toHaveBeenCalledExactlyOnceWith({ isEditable: true, hasSelection: true });
  });

  it("입력창에 커서만 있으면 붙여넣기는 되지만 복사할 것은 없다고 알린다", () => {
    const showTextContextMenu = openWithBridge();
    const draft = screen.getByLabelText<HTMLTextAreaElement>("draft");
    draft.setSelectionRange(2, 2);

    fireEvent.contextMenu(draft);

    expect(showTextContextMenu).toHaveBeenCalledExactlyOnceWith({ isEditable: true, hasSelection: false });
  });

  it("체크박스는 글자 입력창으로 취급하지 않는다", () => {
    const showTextContextMenu = openWithBridge();

    fireEvent.contextMenu(screen.getByLabelText("option"));

    expect(showTextContextMenu).toHaveBeenCalledExactlyOnceWith({ isEditable: false, hasSelection: false });
  });

  it("데스크톱 브리지가 없으면 브라우저 기본 메뉴를 막지 않는다", () => {
    render(<Surface />);

    const wasDefaultAllowed = fireEvent.contextMenu(screen.getByText("선택할 글자"));

    expect(wasDefaultAllowed).toBe(true);
  });
});
