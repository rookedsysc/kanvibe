import { useEffect, type ReactNode } from "react";
import { fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import { BoardCommandProvider, useBoardCommands } from "@/desktop/renderer/components/BoardCommandProvider";
import CommandPaletteDialog from "@/desktop/renderer/components/CommandPaletteDialog";
import { TaskStatus } from "@/entities/KanbanTask";

const mocks = vi.hoisted(() => ({
  runBackgroundTaskSyncNow: vi.fn(),
}));

vi.mock("next-intl", () => ({
  useTranslations: (namespace: string) => (key: string, values?: Record<string, string>) => (
    values?.status ? `${namespace}.${key}:${values.status}` : `${namespace}.${key}`
  ),
}));

vi.mock("@/desktop/renderer/actions/backgroundTaskSync", () => ({
  runBackgroundTaskSyncNow: (...args: unknown[]) => mocks.runBackgroundTaskSyncNow(...args),
}));

function ActiveTaskContextHarness({
  moveTaskToStatus,
}: {
  moveTaskToStatus: (status: TaskStatus) => void;
}) {
  const boardCommands = useBoardCommands();

  useEffect(() => boardCommands.registerActiveTaskContext({
    taskId: "task-1",
    currentStatus: TaskStatus.TODO,
    moveTaskToStatus,
  }), [boardCommands, moveTaskToStatus]);

  return null;
}

function BoardMoveHandlerHarness({
  moveFocusedTaskToStatus,
}: {
  moveFocusedTaskToStatus: (status: TaskStatus, taskIdOverride?: string | null) => "moved" | "missing-focus" | "missing-task";
}) {
  const boardCommands = useBoardCommands();

  useEffect(() => boardCommands.registerBoardHandlers({
    toggleNotificationCenter: () => {},
    openProjectFilter: () => {},
    openCreateTaskModal: () => {},
    moveFocusedTaskToStatus,
  }), [boardCommands, moveFocusedTaskToStatus]);

  return null;
}

function renderWithRouter(children: ReactNode) {
  return render(
    <MemoryRouter initialEntries={["/ko"]}>
      {children}
    </MemoryRouter>,
  );
}

function openPalette() {
  fireEvent.keyDown(window, { key: "p", ctrlKey: true, shiftKey: true });
}

function getSearchInput() {
  return screen.getByRole("textbox");
}

describe("CommandPaletteDialog", () => {
  afterEach(() => {
    vi.clearAllMocks();
  });

  it("Move 대상이 없으면 Move 관련 행이 하나도 렌더되지 않는다", () => {
    renderWithRouter(
      <BoardCommandProvider>
        <CommandPaletteDialog />
      </BoardCommandProvider>,
    );

    openPalette();

    expect(screen.queryByRole("button", { name: /commandPalette\.moveToStatusLabel/ })).toBeNull();
    expect(screen.getByRole("button", { name: /commandPalette\.syncLabel/ })).toBeTruthy();
  });

  it("Sync 행을 클릭하면 백그라운드 동기화를 정확히 1회 호출하고 다이얼로그가 닫힌다", async () => {
    mocks.runBackgroundTaskSyncNow.mockResolvedValue(undefined);

    renderWithRouter(
      <BoardCommandProvider>
        <CommandPaletteDialog />
      </BoardCommandProvider>,
    );

    openPalette();
    fireEvent.click(screen.getByRole("button", { name: /commandPalette\.syncLabel/ }));

    expect(mocks.runBackgroundTaskSyncNow).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("task 상세 컨텍스트가 있으면 'Move to Review' 행 클릭이 그 task를 이동한다", () => {
    const moveTaskToStatus = vi.fn();

    renderWithRouter(
      <BoardCommandProvider>
        <ActiveTaskContextHarness moveTaskToStatus={moveTaskToStatus} />
        <CommandPaletteDialog />
      </BoardCommandProvider>,
    );

    openPalette();
    fireEvent.click(screen.getByRole("button", { name: /board\.columns\.review/ }));

    expect(moveTaskToStatus).toHaveBeenCalledTimes(1);
    expect(moveTaskToStatus).toHaveBeenCalledWith(TaskStatus.REVIEW);
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("task 컨텍스트가 없어도 보드에 포커스된 태스크가 있으면 board의 이동 핸들러를 그 태스크 id로 호출한다", () => {
    const moveFocusedTaskToStatus = vi.fn().mockReturnValue("moved");

    renderWithRouter(
      <BoardCommandProvider>
        <BoardMoveHandlerHarness moveFocusedTaskToStatus={moveFocusedTaskToStatus} />
        <a href="#" data-kanban-task-card="true" data-kanban-task-id="task-9" tabIndex={0}>
          focused card
        </a>
        <CommandPaletteDialog />
      </BoardCommandProvider>,
    );

    screen.getByText("focused card").focus();
    openPalette();
    fireEvent.click(screen.getByRole("button", { name: /board\.columns\.done/ }));

    expect(moveFocusedTaskToStatus).toHaveBeenCalledTimes(1);
    expect(moveFocusedTaskToStatus).toHaveBeenCalledWith(TaskStatus.DONE, "task-9");
  });

  it("검색어를 입력하면 매칭되는 행으로만 목록이 좁혀진다", () => {
    const moveTaskToStatus = vi.fn();

    renderWithRouter(
      <BoardCommandProvider>
        <ActiveTaskContextHarness moveTaskToStatus={moveTaskToStatus} />
        <CommandPaletteDialog />
      </BoardCommandProvider>,
    );

    openPalette();
    fireEvent.change(getSearchInput(), { target: { value: "sync" } });

    expect(screen.getByRole("button", { name: /commandPalette\.syncLabel/ })).toBeTruthy();
    expect(screen.queryByRole("button", { name: /board\.columns\.review/ })).toBeNull();
  });

  it("각 행은 명령어(command)와 설명(description)을 분리된 요소로 렌더링한다", () => {
    const moveTaskToStatus = vi.fn();

    renderWithRouter(
      <BoardCommandProvider>
        <ActiveTaskContextHarness moveTaskToStatus={moveTaskToStatus} />
        <CommandPaletteDialog />
      </BoardCommandProvider>,
    );

    openPalette();

    expect(screen.getByText("sync")).toBeTruthy();
    expect(screen.getByText("commandPalette.syncLabel")).toBeTruthy();
    expect(screen.getByText("move review")).toBeTruthy();
    expect(screen.getByText(/commandPalette\.moveToStatusLabel:board\.columns\.review/)).toBeTruthy();
  });

  it("명령어 토큰(예: 'move progress')으로 검색하면 해당 행만 남는다", () => {
    const moveTaskToStatus = vi.fn();

    renderWithRouter(
      <BoardCommandProvider>
        <ActiveTaskContextHarness moveTaskToStatus={moveTaskToStatus} />
        <CommandPaletteDialog />
      </BoardCommandProvider>,
    );

    openPalette();
    fireEvent.change(getSearchInput(), { target: { value: "move progress" } });

    expect(screen.getByText("move progress")).toBeTruthy();
    expect(screen.queryByText("move review")).toBeNull();
    expect(screen.queryByText("sync")).toBeNull();
  });

  it("ArrowDown 후 Enter는 두 번째 행을 실행한다", () => {
    const moveTaskToStatus = vi.fn();

    renderWithRouter(
      <BoardCommandProvider>
        <ActiveTaskContextHarness moveTaskToStatus={moveTaskToStatus} />
        <CommandPaletteDialog />
      </BoardCommandProvider>,
    );

    openPalette();
    const input = getSearchInput();
    fireEvent.keyDown(input, { key: "ArrowDown" });
    fireEvent.keyDown(input, { key: "Enter" });

    expect(mocks.runBackgroundTaskSyncNow).not.toHaveBeenCalled();
    expect(moveTaskToStatus).toHaveBeenCalledTimes(1);
    expect(moveTaskToStatus).toHaveBeenCalledWith(TaskStatus.PROGRESS);
  });

  it("매칭되는 행이 없으면 빈 상태 문구를 보여준다", () => {
    renderWithRouter(
      <BoardCommandProvider>
        <CommandPaletteDialog />
      </BoardCommandProvider>,
    );

    openPalette();
    fireEvent.change(getSearchInput(), { target: { value: "존재하지않는명령" } });

    expect(screen.getByText("commandPalette.empty")).toBeTruthy();
  });
});
