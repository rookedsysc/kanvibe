import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import PaneLayoutRoute from "@/desktop/renderer/routes/PaneLayoutRoute";
import { triggerDesktopRefresh } from "@/desktop/renderer/utils/refresh";
import type { PaneLayoutConfig } from "@/entities/PaneLayoutConfig";

const mocks = vi.hoisted(() => ({
  global: vi.fn(), projects: vi.fn(), projectConfig: vi.fn(), save: vi.fn(),
}));
vi.mock("next-intl", () => ({ useTranslations: () => (key: string) => key }));
vi.mock("@/desktop/renderer/navigation", () => ({
  Link: ({ children }: { children: React.ReactNode }) => <span>{children}</span>,
}));
vi.mock("@/desktop/renderer/actions/project", () => ({ getAllProjects: mocks.projects }));
vi.mock("@/desktop/renderer/actions/paneLayout", () => ({
  getGlobalPaneLayout: mocks.global,
  getProjectPaneLayout: mocks.projectConfig,
  savePaneLayout: mocks.save,
  deletePaneLayout: vi.fn(),
}));

const stored = {
  id: "global", layoutType: "quad",
  panes: Array.from({ length: 4 }, (_, position) => ({ position, command: `command-${position}` })),
} as PaneLayoutConfig;
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((complete) => { resolve = complete; });
  return { promise, resolve };
}
async function settle() { await act(async () => {}); }

describe("PaneLayoutRoute read safety", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    mocks.global.mockResolvedValue(null);
    mocks.projects.mockResolvedValue([]);
    mocks.save.mockResolvedValue(undefined);
    vi.spyOn(console, "error").mockImplementation(() => {});
  });
  afterEach(() => { cleanup(); vi.useRealTimers(); vi.restoreAllMocks(); });

  it("does not mount an empty editor after timeout and hydrates the delayed saved layout", async () => {
    vi.useFakeTimers();
    const pending = deferred<PaneLayoutConfig | null>();
    mocks.global.mockReturnValue(pending.promise);
    render(<PaneLayoutRoute />);
    await act(async () => { vi.advanceTimersByTime(5000); });
    expect(screen.queryAllByRole("textbox")).toHaveLength(0);
    expect(screen.queryByRole("button", { name: "save" })).toBeNull();
    await act(async () => { pending.resolve(stored); });
    expect(screen.getAllByRole("textbox").map((input) => (input as HTMLInputElement).value))
      .toEqual(stored.panes.map((pane) => pane.command));
    fireEvent.click(screen.getByRole("button", { name: "save" }));
    await settle();
    expect(mocks.save).toHaveBeenCalledWith(expect.objectContaining({ layoutType: "quad", panes: stored.panes }));
  });

  it("keeps an initial global failure unsavable and retries it", async () => {
    mocks.global.mockRejectedValueOnce(new Error("offline")).mockResolvedValueOnce(stored);
    render(<PaneLayoutRoute />);
    await settle();
    expect(screen.queryByRole("button", { name: "save" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "retry" }));
    await settle();
    expect(screen.getAllByRole("textbox")).toHaveLength(4);
  });

  it("does not call a failed project read an absent override and retries it", async () => {
    mocks.projects.mockResolvedValue([{ id: "project-A", name: "Project A" }]);
    mocks.projectConfig.mockRejectedValueOnce(new Error("offline")).mockResolvedValueOnce(stored);
    render(<PaneLayoutRoute />);
    await settle();
    expect(screen.queryByText("usingGlobal")).toBeNull();
    expect(screen.getAllByRole("button", { name: "save", hidden: true })).toHaveLength(1);
    fireEvent.click(screen.getByRole("button", { name: "retry", hidden: true }));
    await settle();
    expect(screen.getByText("hasOverride")).toBeTruthy();
    expect(screen.getAllByRole("textbox", { hidden: true })).toHaveLength(5);
  });

  it("allows an explicitly absent global setting to be created", async () => {
    render(<PaneLayoutRoute />);
    await settle();
    fireEvent.click(screen.getByRole("button", { name: "save" }));
    await settle();
    expect(mocks.save).toHaveBeenCalledWith({
      layoutType: "single", panes: [{ position: 0, command: "" }], projectId: null, isGlobal: true,
    });
  });

  it("preserves an edited draft through successful and failed refreshes", async () => {
    mocks.global.mockResolvedValue(stored);
    render(<PaneLayoutRoute />);
    await settle();
    fireEvent.change(screen.getAllByRole("textbox")[0], { target: { value: "unsaved draft" } });
    act(() => triggerDesktopRefresh("pane-layout"));
    await settle();
    expect((screen.getAllByRole("textbox")[0] as HTMLInputElement).value).toBe("unsaved draft");
    mocks.global.mockRejectedValueOnce(new Error("offline"));
    act(() => triggerDesktopRefresh("pane-layout"));
    await settle();
    expect((screen.getAllByRole("textbox")[0] as HTMLInputElement).value).toBe("unsaved draft");
  });

  it("keeps a successfully loaded project draft when its refresh fails", async () => {
    mocks.projects.mockResolvedValue([{ id: "project-A", name: "Project A" }]);
    mocks.projectConfig.mockResolvedValueOnce(stored).mockRejectedValueOnce(new Error("offline"));
    render(<PaneLayoutRoute />);
    await settle();
    fireEvent.change(screen.getAllByRole("textbox", { hidden: true })[1], { target: { value: "project draft" } });
    act(() => triggerDesktopRefresh("pane-layout"));
    await settle();
    expect(screen.getByText("hasOverride")).toBeTruthy();
    expect((screen.getAllByRole("textbox", { hidden: true })[1] as HTMLInputElement).value).toBe("project draft");
  });

});
