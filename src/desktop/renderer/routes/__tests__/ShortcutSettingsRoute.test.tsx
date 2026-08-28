import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { BoardCommandProvider } from "@/desktop/renderer/components/BoardCommandProvider";
import ShortcutSettingsRoute from "@/desktop/renderer/routes/ShortcutSettingsRoute";
import { DEFAULT_SHORTCUT_BINDINGS } from "@/desktop/shared/shortcutBindings";

const mocks = vi.hoisted(() => ({
  getShortcutBindings: vi.fn(),
  setShortcutBindings: vi.fn(),
  notifyShortcutBindingsChanged: vi.fn(),
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
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.getShortcutBindings.mockResolvedValue({ ...DEFAULT_SHORTCUT_BINDINGS });
    mocks.setShortcutBindings.mockResolvedValue(undefined);
    window.kanvibeDesktop = {
      isDesktop: true,
      notifyShortcutBindingsChanged: mocks.notifyShortcutBindingsChanged,
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

  it("전체 기본값으로 되돌리면 저장된 재배정을 모두 지운다", async () => {
    renderShortcutSettings();

    fireEvent.click(screen.getByText("resetAll"));

    await waitFor(() => {
      expect(mocks.setShortcutBindings).toHaveBeenCalledWith(DEFAULT_SHORTCUT_BINDINGS);
    });
  });
});
