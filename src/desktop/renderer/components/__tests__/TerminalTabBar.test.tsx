import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import TerminalTabBar from "../TerminalTabBar";
import type { TerminalTab } from "@/desktop/shared/terminalTabs";

vi.mock("next-intl", () => ({
  useTranslations: () => (key: string, values?: Record<string, string>) => (
    values ? `${key}:${Object.values(values).join(",")}` : key
  ),
}));

const TABS: TerminalTab[] = [
  { id: "@1", index: 0, name: "shell", isActive: true },
  { id: "@2", index: 1, name: "logs", isActive: false },
];

function renderTabBar(overrides: Partial<Parameters<typeof TerminalTabBar>[0]> = {}) {
  const handlers = {
    onSelect: vi.fn(),
    onCreate: vi.fn(),
    onClose: vi.fn(),
    onRename: vi.fn(),
    onMove: vi.fn(),
  };

  render(<TerminalTabBar tabs={TABS} {...handlers} {...overrides} />);
  return handlers;
}

describe("TerminalTabBar", () => {
  it("활성 탭을 aria-selected로 표시한다", () => {
    renderTabBar();

    const [firstTab, secondTab] = screen.getAllByRole("tab");
    expect(firstTab.getAttribute("aria-selected")).toBe("true");
    expect(secondTab.getAttribute("aria-selected")).toBe("false");
  });

  it("비활성 탭을 누르면 선택 콜백이 호출된다", () => {
    const handlers = renderTabBar();

    fireEvent.click(screen.getByText("logs"));

    expect(handlers.onSelect).toHaveBeenCalledWith("@2");
  });

  it("이미 활성인 탭을 눌러도 선택 콜백을 부르지 않는다", () => {
    const handlers = renderTabBar();

    fireEvent.click(screen.getByText("shell"));

    expect(handlers.onSelect).not.toHaveBeenCalled();
  });

  it("탭의 닫기 버튼은 선택으로 번지지 않는다", () => {
    const handlers = renderTabBar();

    fireEvent.click(screen.getByLabelText("closeTerminalTab:logs"));

    expect(handlers.onClose).toHaveBeenCalledWith("@2");
    expect(handlers.onSelect).not.toHaveBeenCalled();
  });

  it("새 탭 버튼이 생성 콜백을 부른다", () => {
    const handlers = renderTabBar();

    fireEvent.click(screen.getByLabelText("newTerminalTab"));

    expect(handlers.onCreate).toHaveBeenCalled();
  });

  it("더블클릭 후 Enter로 이름을 바꾼다", () => {
    const handlers = renderTabBar();

    fireEvent.doubleClick(screen.getByText("logs"));
    const renameInput = screen.getByLabelText("renameTerminalTab");
    fireEvent.change(renameInput, { target: { value: "build" } });
    fireEvent.keyDown(renameInput, { key: "Enter" });

    expect(handlers.onRename).toHaveBeenCalledWith("@2", "build");
  });

  it("Esc로 이름 변경을 취소하면 콜백을 부르지 않는다", () => {
    const handlers = renderTabBar();

    fireEvent.doubleClick(screen.getByText("logs"));
    const renameInput = screen.getByLabelText("renameTerminalTab");
    fireEvent.change(renameInput, { target: { value: "build" } });
    fireEvent.keyDown(renameInput, { key: "Escape" });

    expect(handlers.onRename).not.toHaveBeenCalled();
    expect(screen.getByText("logs")).toBeTruthy();
  });

  it("이름을 그대로 두고 빠져나오면 변경 요청을 보내지 않는다", () => {
    const handlers = renderTabBar();

    fireEvent.doubleClick(screen.getByText("logs"));
    fireEvent.blur(screen.getByLabelText("renameTerminalTab"));

    expect(handlers.onRename).not.toHaveBeenCalled();
  });

  it("드래그해서 놓은 위치의 인덱스로 이동을 요청한다", () => {
    const handlers = renderTabBar();
    const [firstTab, secondTab] = screen.getAllByRole("tab");

    fireEvent.dragStart(firstTab);
    fireEvent.drop(secondTab);

    expect(handlers.onMove).toHaveBeenCalledWith("@1", 1);
  });

  it("같은 탭 위에 놓으면 이동을 요청하지 않는다", () => {
    const handlers = renderTabBar();
    const [firstTab] = screen.getAllByRole("tab");

    fireEvent.dragStart(firstTab);
    fireEvent.drop(firstTab);

    expect(handlers.onMove).not.toHaveBeenCalled();
  });
});
