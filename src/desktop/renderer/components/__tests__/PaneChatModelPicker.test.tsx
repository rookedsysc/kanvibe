import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { PaneChatModelPicker } from "../PaneChatModelPicker";
import type { PaneChatModelSettings } from "@/lib/aiSessions/paneChatModelSettings";

const actions = vi.hoisted(() => ({ getPaneChatModelSettings: vi.fn(), changePaneChatModelSetting: vi.fn() }));
vi.mock("@/desktop/renderer/actions/paneChat", () => actions);
vi.mock("next-intl", () => ({ useTranslations: () => (key: string) => key }));
const settings: PaneChatModelSettings = {
  model: "a", effort: "high", isFast: false,
  models: [{ id: "a", label: "Alpha", supportsFast: true, effortLevels: [{ id: "low", label: "Light" }, { id: "high", label: "High" }] }, { id: "b", label: "Beta", supportsFast: false, effortLevels: [] }],
};
function open(initialSettings?: PaneChatModelSettings) {
  const onClose = vi.fn();
  const onChanged = vi.fn();
  return { ...render(<PaneChatModelPicker taskId="task" targetToken="pane-a" initialSettings={initialSettings} onClose={onClose} onChanged={onChanged} />), onClose, onChanged };
}
beforeEach(() => {
  vi.clearAllMocks();
  actions.getPaneChatModelSettings.mockResolvedValue(settings);
  actions.changePaneChatModelSetting.mockResolvedValue({ success: true });
});
afterEach(cleanup);

it("renders snapshot options immediately and refreshes through the API", async () => {
  let complete!: (value: PaneChatModelSettings) => void;
  actions.getPaneChatModelSettings.mockReturnValue(new Promise((resolve) => { complete = resolve; }));
  open(settings);
  expect(screen.getByRole("menuitemradio", { name: "Alpha", checked: true })).toBe(document.activeElement);
  expect(screen.queryByText("loading")).toBeNull();
  await act(async () => complete({ ...settings, model: "b" }));
  expect(screen.getByRole("menuitemradio", { name: "Beta", checked: true })).toBeTruthy();
});

it("navigates models without applying until chosen and dismisses with Escape or outside pointer", async () => {
  const view = open();
  const first = await screen.findByRole("menuitemradio", { name: "Alpha" });
  first.focus();
  fireEvent.keyDown(first, { key: "ArrowDown" });
  expect(document.activeElement).toBe(screen.getByRole("menuitemradio", { name: "Beta" }));
  expect(actions.changePaneChatModelSetting).not.toHaveBeenCalled();
  fireEvent.keyDown(document.activeElement!, { key: "Escape" });
  expect(view.onClose).toHaveBeenCalledTimes(1);
  fireEvent.pointerDown(document.body);
  expect(view.onClose).toHaveBeenCalledTimes(2);
});

it("applies effort, reset and fast settings and waits for confirmed state", async () => {
  open();
  fireEvent.click(await screen.findByRole("button", { name: "Light" }));
  await waitFor(() => expect(actions.changePaneChatModelSetting).toHaveBeenCalledWith("task", "pane-a", { kind: "effort", effort: "low" }));
  await waitFor(() => expect(screen.getByRole("dialog").getAttribute("aria-busy")).toBe("false"));
  fireEvent.click(screen.getByRole("button", { name: "defaultEffort" }));
  await waitFor(() => expect(actions.changePaneChatModelSetting).toHaveBeenCalledWith("task", "pane-a", { kind: "reset" }));
  await waitFor(() => expect(screen.getByRole("dialog").getAttribute("aria-busy")).toBe("false"));
  fireEvent.click(screen.getByRole("switch", { name: "fastMode" }));
  await waitFor(() => expect(actions.changePaneChatModelSetting).toHaveBeenCalledWith("task", "pane-a", { kind: "fast", isFast: true }));
  expect(screen.getByRole("switch", { name: "fastMode" }).getAttribute("aria-checked")).toBe("false");
});

it("retains confirmed selection after failed updates and retries unavailable catalogs", async () => {
  actions.getPaneChatModelSettings.mockRejectedValueOnce(new Error("offline"));
  open(settings);
  await screen.findByRole("alert");
  fireEvent.click(screen.getByRole("button", { name: "retry" }));
  await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
  actions.changePaneChatModelSetting.mockRejectedValueOnce(new Error("rejected"));
  fireEvent.click(screen.getByRole("menuitemradio", { name: "Beta" }));
  await screen.findByRole("alert");
  expect(screen.getByRole("menuitemradio", { name: "Alpha", checked: true })).toBeTruthy();
});

it("does not refresh or notify a disposed picker after a model update completes", async () => {
  let complete!: () => void;
  actions.changePaneChatModelSetting.mockReturnValue(new Promise<void>((resolve) => { complete = resolve; }));
  const view = open();
  fireEvent.click(await screen.findByRole("menuitemradio", { name: "Beta" }));
  view.unmount();
  await act(async () => complete());
  expect(actions.getPaneChatModelSettings).toHaveBeenCalledTimes(1);
  expect(view.onChanged).not.toHaveBeenCalled();
});

it("ignores an older catalog response after a setting update is confirmed", async () => {
  let finishInitial!: (value: PaneChatModelSettings) => void;
  actions.getPaneChatModelSettings.mockReturnValueOnce(new Promise((resolve) => { finishInitial = resolve; })).mockResolvedValue({ ...settings, model: "b" });
  const view = open(settings);
  fireEvent.click(screen.getByRole("menuitemradio", { name: "Beta" }));
  await waitFor(() => expect(view.onChanged).toHaveBeenCalledTimes(1));
  await act(async () => finishInitial(settings));
  expect(screen.getByRole("menuitemradio", { name: "Beta", checked: true })).toBeTruthy();
});
