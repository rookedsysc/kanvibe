import { afterEach, describe, expect, it, vi } from "vitest";

const { isTauri, confirm, message } = vi.hoisted(() => ({
  isTauri: vi.fn(() => true),
  confirm: vi.fn(),
  message: vi.fn(),
}));
vi.mock("@tauri-apps/api/core", () => ({ isTauri }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ confirm, message }));
import { confirmAction, showAlert } from "../dialogs";

afterEach(() => { vi.restoreAllMocks(); vi.clearAllMocks(); isTauri.mockReturnValue(true); });

describe("desktop dialogs", () => {
  it.each([true, false])("native confirmation preserves the user's %s decision", async decision => {
    const browser = vi.spyOn(window, "confirm").mockReturnValue(false);
    confirm.mockResolvedValueOnce(decision);
    expect(await confirmAction("running: sleep")).toBe(decision);
    expect(confirm).toHaveBeenCalledWith("running: sleep", { title: "KanVibe" });
    expect(browser).not.toHaveBeenCalled();
  });
  it("browser confirmation still uses the browser decision", async () => {
    isTauri.mockReturnValue(false);
    const browser = vi.spyOn(window, "confirm").mockReturnValue(true);
    expect(await confirmAction("delete task?")).toBe(true);
    expect(browser).toHaveBeenCalledWith("delete task?");
    expect(confirm).not.toHaveBeenCalled();
  });
  it("native alerts await the message dialog", async () => {
    const browser = vi.spyOn(window, "alert").mockImplementation(() => {});
    message.mockResolvedValueOnce(undefined);
    await showAlert("process lookup failed");
    expect(message).toHaveBeenCalledWith("process lookup failed", { title: "KanVibe" });
    expect(browser).not.toHaveBeenCalled();
  });
  it("browser alerts retain their message", async () => {
    isTauri.mockReturnValue(false);
    const browser = vi.spyOn(window, "alert").mockImplementation(() => {});
    await showAlert("process lookup failed");
    expect(browser).toHaveBeenCalledWith("process lookup failed");
    expect(message).not.toHaveBeenCalled();
  });
});
