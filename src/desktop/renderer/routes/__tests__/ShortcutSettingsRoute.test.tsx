import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { BoardCommandProvider } from "@/desktop/renderer/components/BoardCommandProvider";
import ShortcutSettingsRoute from "@/desktop/renderer/routes/ShortcutSettingsRoute";
import { loadShortcutBindings } from "@/desktop/renderer/utils/shortcutBindings";
import { DEFAULT_SHORTCUT_BINDINGS } from "@/desktop/shared/shortcutBindings";

const mocks = vi.hoisted(() => ({
  getShortcutBindings: vi.fn(),
  setShortcutBindings: vi.fn(),
  notifyShortcutBindingsChanged: vi.fn(),
  notifyShortcutCaptureChanged: vi.fn(),
}));

vi.mock("next-intl", () => ({
  useTranslations: () => Object.assign(
    (key: string, values?: Record<string, unknown>) => (
      values && "index" in values ? `${key}:${values.index}` : key
    ),
    { rich: (key: string) => key },
  ),
}));

vi.mock("@/desktop/renderer/navigation", () => ({
  Link: ({ children, href }: { children: React.ReactNode; href: string }) => <a href={href}>{children}</a>,
  useRouter: () => ({ back: vi.fn(), forward: vi.fn(), push: vi.fn() }),
}));

vi.mock("@/desktop/renderer/actions/appSettings", () => ({
  getShortcutBindings: (...args: unknown[]) => mocks.getShortcutBindings(...args),
  setShortcutBindings: (...args: unknown[]) => mocks.setShortcutBindings(...args),
}));

function renderShortcutSettings() {
  return render(
    <BoardCommandProvider>
      <ShortcutSettingsRoute />
    </BoardCommandProvider>,
  );
}

function findRecordButtonFor(commandLabel: string) {
  const commandRow = screen.getByText(commandLabel).closest("li");
  if (!commandRow) {
    throw new Error(`단축키 행을 찾지 못했습니다: ${commandLabel}`);
  }

  return commandRow;
}

describe("ShortcutSettingsRoute", () => {
  beforeEach(async () => {
    vi.clearAllMocks();
    mocks.getShortcutBindings.mockResolvedValue({ ...DEFAULT_SHORTCUT_BINDINGS });
    mocks.setShortcutBindings.mockResolvedValue(undefined);
    await loadShortcutBindings();
    window.kanvibeDesktop = {
      isDesktop: true,
      notifyShortcutBindingsChanged: mocks.notifyShortcutBindingsChanged,
      notifyShortcutCaptureChanged: mocks.notifyShortcutCaptureChanged,
    };
  });

  it("녹화한 조합을 그 명령의 새 단축키로 저장한다", async () => {
    renderShortcutSettings();

    const dockRow = findRecordButtonFor("commands.taskDetailDock:4");
    fireEvent.click(within(dockRow).getByText("record"));
    fireEvent.keyDown(window, { key: "k", ctrlKey: true, shiftKey: true });

    await waitFor(() => {
      expect(mocks.setShortcutBindings).toHaveBeenCalledWith(expect.objectContaining({
        taskDetailDock4: "Mod+Shift+K",
      }));
    });

    /** main도 같은 표를 들고 있어야 하므로 저장만 하고 알리지 않으면 Electron 경로가 옛 조합으로 남는다 */
    expect(mocks.notifyShortcutBindingsChanged).toHaveBeenCalled();
  });

  /** 같은 조합이 두 명령에 붙으면 뒤 명령은 영영 실행되지 않는다 */
  it("이미 쓰는 조합은 저장하지 않고 어떤 명령과 겹치는지 알린다", async () => {
    renderShortcutSettings();

    const dockRow = findRecordButtonFor("commands.taskDetailDock:1");
    fireEvent.click(within(dockRow).getByText("record"));
    fireEvent.keyDown(window, { key: "n", ctrlKey: true });

    expect(await screen.findByRole("alert")).toBeTruthy();
    expect(mocks.setShortcutBindings).not.toHaveBeenCalled();
  });

  /**
   * 개별 되돌리기가 충돌 검사를 건너뛰면 되돌린 조합이 다른 명령의 조합과 같아진다.
   * 그러면 정의 순서상 뒤인 명령은 재배정하기 전까지 실행할 방법이 없다.
   */
  it("개별 기본값 되돌리기가 다른 명령과 겹치면 저장하지 않고 알린다", async () => {
    renderShortcutSettings();

    const firstDockRow = findRecordButtonFor("commands.taskDetailDock:1");
    fireEvent.click(within(firstDockRow).getByText("record"));
    fireEvent.keyDown(window, { key: "j", ctrlKey: true, shiftKey: true });
    await waitFor(() => {
      expect(mocks.setShortcutBindings).toHaveBeenCalledWith(expect.objectContaining({
        taskDetailDock1: "Mod+Shift+J",
      }));
    });

    const secondDockRow = findRecordButtonFor("commands.taskDetailDock:2");
    fireEvent.click(within(secondDockRow).getByText("record"));
    fireEvent.keyDown(window, { key: "1", ctrlKey: true });
    await waitFor(() => {
      expect(mocks.setShortcutBindings).toHaveBeenCalledWith(expect.objectContaining({
        taskDetailDock2: "Mod+1",
      }));
    });

    mocks.setShortcutBindings.mockClear();
    fireEvent.click(within(findRecordButtonFor("commands.taskDetailDock:1")).getByText("reset"));

    expect((await screen.findByRole("alert")).textContent).toBe("conflict");
    expect(mocks.setShortcutBindings).not.toHaveBeenCalled();
  });

  it("Esc를 누르면 녹화를 취소한다", async () => {
    renderShortcutSettings();

    const dockRow = findRecordButtonFor("commands.taskDetailDock:1");
    fireEvent.click(within(dockRow).getByText("record"));
    expect(within(dockRow).getByText("recording")).toBeTruthy();

    fireEvent.keyDown(window, { key: "Escape" });

    await waitFor(() => {
      expect(within(dockRow).getByText("record")).toBeTruthy();
    });
    expect(mocks.setShortcutBindings).not.toHaveBeenCalled();
  });

  /**
   * 조합을 누르는 사용자는 Ctrl/Cmd/Shift keydown을 먼저 흘린다.
   * 이걸 "수정 키가 없는 조합"과 같이 다루면 모든 녹화 시도마다 사실과 반대인 오류가 뜨고,
   * role="alert"라 스크린리더는 수식키를 누를 때마다 거짓 오류를 읽는다.
   */
  it("수식 키만 눌린 동안에는 오류를 띄우지 않는다", async () => {
    renderShortcutSettings();

    const dockRow = findRecordButtonFor("commands.taskDetailDock:1");
    fireEvent.click(within(dockRow).getByText("record"));
    fireEvent.keyDown(window, { key: "Control", ctrlKey: true });

    expect(screen.queryByRole("alert")).toBeNull();
    expect(within(dockRow).getByText("recording")).toBeTruthy();
  });

  /** 배정할 수 없는 키를 조용히 버리면 사용자는 재배정이 됐다고 믿고 화면을 떠난다 */
  it("담을 수 없는 조합을 녹화하면 저장하지 않고 오류를 알린다", async () => {
    renderShortcutSettings();

    const dockRow = findRecordButtonFor("commands.taskDetailDock:1");
    fireEvent.click(within(dockRow).getByText("record"));
    fireEvent.keyDown(window, { key: "+", ctrlKey: true, shiftKey: true });

    expect((await screen.findByRole("alert")).textContent).toBe("unsupported");
    expect(mocks.setShortcutBindings).not.toHaveBeenCalled();
  });

  /** 녹화를 끝낸 화면에 옛 오류가 남으면 사용자는 방금 한 취소가 실패한 줄 안다 */
  it("Esc로 취소하면 남아 있던 오류 배너를 지운다", async () => {
    renderShortcutSettings();

    const dockRow = findRecordButtonFor("commands.taskDetailDock:1");
    fireEvent.click(within(dockRow).getByText("record"));
    fireEvent.keyDown(window, { key: "n", ctrlKey: true });
    expect((await screen.findByRole("alert")).textContent).toBe("conflict");

    fireEvent.keyDown(window, { key: "Escape" });

    await waitFor(() => {
      expect(screen.queryByRole("alert")).toBeNull();
    });
  });

  it("전체 기본값으로 되돌리면 저장된 재배정을 모두 지운다", async () => {
    renderShortcutSettings();

    fireEvent.click(screen.getByText("resetAll"));

    await waitFor(() => {
      expect(mocks.setShortcutBindings).toHaveBeenCalledWith(DEFAULT_SHORTCUT_BINDINGS);
    });
  });

  /**
   * main이 녹화 중임을 모르면 `before-input-event`가 조합을 가로채, 그 조합의 원래 동작만 실행되고
   * keydown은 녹화 처리기까지 오지 못한다.
   */
  it("녹화를 시작하고 끝낼 때 Electron main에 알린다", async () => {
    renderShortcutSettings();

    const dockRow = findRecordButtonFor("commands.taskDetailDock:1");
    fireEvent.click(within(dockRow).getByText("record"));
    expect(mocks.notifyShortcutCaptureChanged).toHaveBeenLastCalledWith(true);

    fireEvent.keyDown(window, { key: "Escape" });

    await waitFor(() => {
      expect(mocks.notifyShortcutCaptureChanged).toHaveBeenLastCalledWith(false);
    });
  });

  /** 저장이 실패했는데 조용하면 화면은 옛 값 그대로인 채로 사용자는 바뀐 줄 안다 */
  it("저장이 실패하면 오류 배너로 알린다", async () => {
    const consoleErrorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    mocks.setShortcutBindings.mockRejectedValue(new Error("설정 저장 실패"));
    renderShortcutSettings();

    fireEvent.click(screen.getByText("resetAll"));

    expect((await screen.findByRole("alert")).textContent).toBe("saveFailed");

    consoleErrorSpy.mockRestore();
  });

  /**
   * 저장은 단축키 표를 통째로 치환한다. 조회에 실패한 창의 캐시는 기본값이라,
   * 그 위에서 한 번만 저장해도 저장돼 있던 다른 명령의 재배정이 전부 지워진다.
   */
  it("단축키 조회에 실패하면 재배정을 저장하지 않고 조회 실패를 알린다", async () => {
    const consoleErrorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    mocks.getShortcutBindings.mockRejectedValue(new Error("설정 조회 실패"));
    /** 앱이 뜰 때의 조회가 실패한 창을 그대로 재현한다 */
    await loadShortcutBindings();
    renderShortcutSettings();

    expect((await screen.findByRole("alert")).textContent).toBe("loadFailed");

    const dockRow = findRecordButtonFor("commands.taskDetailDock:4");
    fireEvent.click(within(dockRow).getByText("record"));
    fireEvent.keyDown(window, { key: "k", ctrlKey: true, shiftKey: true });

    await waitFor(() => {
      expect(within(dockRow).getByText("record")).toBeTruthy();
    });
    expect(mocks.setShortcutBindings).not.toHaveBeenCalled();

    consoleErrorSpy.mockRestore();
  });

  /** 나머지 화면은 3개 로케일이 채워져 있는데 제목만 영어 리터럴이면 그 창만 번역이 빠진다 */
  it("문서 제목을 번역 키에서 가져온다", async () => {
    renderShortcutSettings();

    await waitFor(() => {
      expect(document.title).toBe("title");
    });
  });
});
