import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { usePaneChat } from "@/desktop/renderer/hooks/usePaneChat";
import type { PaneChatAttachment, PaneChatSnapshot, PaneChatSuggestions } from "@/desktop/shared/paneChat";
import { PaneChatComposer } from "../PaneChatComposer";

const actions = vi.hoisted(() => ({ stagePaneChatAttachment: vi.fn(), removePaneChatAttachment: vi.fn(), getPaneChatSuggestions: vi.fn() }));
vi.mock("@/desktop/renderer/actions/paneChat", () => actions);
vi.mock("next-intl", () => ({ useTranslations: () => (key: string) => key }));
const attachment: PaneChatAttachment = { id: "image-1", name: "photo.png", kind: "image", mimeType: "image/png", sizeBytes: 4 };
const image = () => new File([new Uint8Array([137, 80, 78, 71])], "photo.png", { type: "image/png" });
const file = () => new File(["hello"], "notes.txt", { type: "text/plain" });
function chat(target = "pane-a") {
  return {
    snapshot: { targetToken: target, taskId: "task", canSend: true, inputState: "ready", provider: "codex", modelPicker: { supported: true }, composer: { attachments: true, skills: true, fileMentions: true, maxAttachments: 10, maxAttachmentBytes: 10_485_760 } } as PaneChatSnapshot,
    readError: false, actionError: null, isReady: true, isBusy: false, send: vi.fn().mockResolvedValue(true), interrupt: vi.fn(),
  } as unknown as ReturnType<typeof usePaneChat>;
}
function open(initial = chat()) {
  let current = initial;
  const props = { taskId: "task", controlsOpen: false, isVisible: true, onModel: vi.fn(), onControls: vi.fn() };
  const view = render(<PaneChatComposer {...props} chat={current} />);
  return { ...view, chat: initial, switchTo(next: ReturnType<typeof usePaneChat>) { current = next; view.rerender(<PaneChatComposer {...props} chat={current} />); } };
}
function select(files: File[]) { fireEvent.change(screen.getByLabelText("attachFiles", { selector: "input" }), { target: { files } }); }
function edit(text: string) { fireEvent.change(screen.getByRole("textbox"), { target: { value: text, selectionStart: text.length } }); }
function suggestions(targetToken = "pane-a"): PaneChatSuggestions {
  return { targetToken, truncated: false, items: [{ id: "skill-1", label: "Review", description: "Review code", insertText: "$review" }, { id: "skill-2", label: "Test", description: "Run tests", insertText: "$test" }] };
}
beforeEach(() => {
  vi.clearAllMocks();
  actions.stagePaneChatAttachment.mockResolvedValue(attachment);
  actions.removePaneChatAttachment.mockResolvedValue({ success: true });
  actions.getPaneChatSuggestions.mockResolvedValue(suggestions());
});
afterEach(cleanup);

describe("pane chat composer", () => {
  it("stages selected bytes and sends an image without text using the opaque ID", async () => {
    const view = open();
    select([image()]);
    await screen.findByAltText("photo.png");
    await waitFor(() => expect((screen.getByRole("button", { name: "send" }) as HTMLButtonElement).disabled).toBe(false));
    expect(actions.stagePaneChatAttachment).toHaveBeenCalledWith("task", "pane-a", { name: "photo.png", mimeType: "image/png", dataUrl: "data:image/png;base64,iVBORw==" });
    fireEvent.click(screen.getByRole("button", { name: "send" }));
    expect(view.chat.send).toHaveBeenCalledWith("", ["image-1"], expect.any(String));
    await waitFor(() => expect(screen.queryByAltText("photo.png")).toBeNull());
  });

  it("accepts photo selection, clipboard images, and dropped files", async () => {
    actions.stagePaneChatAttachment.mockImplementation(async (_task, _token, input) => ({ ...attachment, id: input.name, name: input.name }));
    open();
    fireEvent.change(screen.getByLabelText("attachPhotos", { selector: "input" }), { target: { files: [image()] } });
    await waitFor(() => expect(actions.stagePaneChatAttachment).toHaveBeenCalledTimes(1));
    fireEvent.paste(screen.getByRole("textbox"), { clipboardData: { files: [image()] } });
    fireEvent.drop(screen.getByTestId("pane-chat-composer"), { dataTransfer: { files: [file()] } });
    await waitFor(() => expect(actions.stagePaneChatAttachment).toHaveBeenCalledTimes(3));
    expect(screen.getByText("notes.txt")).toBeTruthy();
  });

  it("retains failed uploads, blocks sending, and retries the same file", async () => {
    actions.stagePaneChatAttachment.mockRejectedValueOnce(new Error("Upload unavailable"));
    open();
    edit("Keep this caption");
    select([image()]);
    await screen.findByText("Upload unavailable");
    expect((screen.getByRole("button", { name: "send" }) as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: "retryAttachment photo.png" }));
    await waitFor(() => expect(screen.queryByText("Upload unavailable")).toBeNull());
    await waitFor(() => expect((screen.getByRole("button", { name: "send" }) as HTMLButtonElement).disabled).toBe(false));
    expect((screen.getByRole("textbox") as HTMLTextAreaElement).value).toBe("Keep this caption");
    expect(actions.stagePaneChatAttachment).toHaveBeenCalledTimes(2);
  });

  it("removes a pending upload after its staging acknowledgement", async () => {
    let resolve!: (value: PaneChatAttachment) => void;
    actions.stagePaneChatAttachment.mockReturnValue(new Promise((accept) => { resolve = accept; }));
    open(); select([image()]);
    await waitFor(() => expect(actions.stagePaneChatAttachment).toHaveBeenCalledTimes(1));
    fireEvent.click(screen.getByRole("button", { name: "removeAttachment photo.png" }));
    await act(async () => resolve(attachment));
    await waitFor(() => expect(screen.queryByText("photo.png")).toBeNull());
    expect(actions.removePaneChatAttachment).toHaveBeenCalledWith("task", "pane-a", "image-1");
  });

  it("keeps an attachment when removal fails and permits retrying removal", async () => {
    actions.removePaneChatAttachment.mockRejectedValueOnce(new Error("Remove unavailable"));
    open(); select([image()]);
    await waitFor(() => expect((screen.getByRole("button", { name: "send" }) as HTMLButtonElement).disabled).toBe(false));
    fireEvent.click(screen.getByRole("button", { name: "removeAttachment photo.png" }));
    await screen.findByText("Remove unavailable");
    fireEvent.click(screen.getByRole("button", { name: "removeAttachment photo.png" }));
    await waitFor(() => expect(screen.queryByText("photo.png")).toBeNull());
  });

  it("isolates pending uploads and drafts when switching to another pane", async () => {
    let resolve!: (value: PaneChatAttachment) => void;
    actions.stagePaneChatAttachment.mockReturnValue(new Promise((accept) => { resolve = accept; }));
    const view = open(); edit("Pane A caption"); select([image()]);
    await waitFor(() => expect(actions.stagePaneChatAttachment).toHaveBeenCalledTimes(1));
    view.switchTo(chat("pane-b")); edit("Pane B caption");
    await act(async () => resolve(attachment));
    expect(screen.queryByText("photo.png")).toBeNull();
    expect((screen.getByRole("textbox") as HTMLTextAreaElement).value).toBe("Pane B caption");
    view.switchTo(view.chat);
    expect((screen.getByRole("textbox") as HTMLTextAreaElement).value).toBe("Pane A caption");
    expect(screen.getByText("photo.png")).toBeTruthy();
  });

  it("preserves newer text and new attachments when an earlier send completes", async () => {
    let resolve!: (value: boolean) => void;
    const current = chat(); vi.mocked(current.send).mockReturnValue(new Promise((accept) => { resolve = accept; }));
    actions.stagePaneChatAttachment.mockResolvedValueOnce(attachment).mockResolvedValueOnce({ ...attachment, id: "file-2", name: "notes.txt", kind: "file" });
    open(current); edit("Original"); select([image()]);
    await waitFor(() => expect((screen.getByRole("button", { name: "send" }) as HTMLButtonElement).disabled).toBe(false));
    fireEvent.click(screen.getByRole("button", { name: "send" }));
    edit("New text"); select([file()]);
    await waitFor(() => expect(actions.stagePaneChatAttachment).toHaveBeenCalledTimes(2));
    await act(async () => resolve(true));
    expect((screen.getByRole("textbox") as HTMLTextAreaElement).value).toBe("New text");
    expect(screen.getByText("notes.txt")).toBeTruthy();
    expect(screen.queryByText("photo.png")).toBeNull();
  });

  it("retains failed sends and reuses the request ID only for an unchanged retry", async () => {
    const view = open(); vi.mocked(view.chat.send).mockResolvedValue(false);
    edit("Retry safely");
    fireEvent.click(screen.getByRole("button", { name: "send" }));
    await screen.findByText("sendError");
    fireEvent.click(screen.getByRole("button", { name: "send" }));
    await waitFor(() => expect(view.chat.send).toHaveBeenCalledTimes(2));
    expect(vi.mocked(view.chat.send).mock.calls[0][2]).toBe(vi.mocked(view.chat.send).mock.calls[1][2]);
    await waitFor(() => expect((screen.getByRole("button", { name: "send" }) as HTMLButtonElement).disabled).toBe(false));
    edit("New request"); fireEvent.click(screen.getByRole("button", { name: "send" }));
    expect(vi.mocked(view.chat.send).mock.calls[2][2]).not.toBe(vi.mocked(view.chat.send).mock.calls[0][2]);
    await screen.findByText("sendError");
  });

  it("searches caret tokens and selects suggestions with arrows and Tab without sending", async () => {
    const view = open(); edit("Please $re");
    await screen.findByRole("option", { name: "Review" });
    expect(actions.getPaneChatSuggestions).toHaveBeenLastCalledWith("task", "pane-a", "skill", "re");
    fireEvent.keyDown(screen.getByRole("textbox"), { key: "ArrowDown" });
    fireEvent.keyDown(screen.getByRole("textbox"), { key: "Tab" });
    expect((screen.getByRole("textbox") as HTMLTextAreaElement).value).toBe("Please $test ");
    expect(view.chat.send).not.toHaveBeenCalled();
  });

  it("supports skill button and pointer selection, command/file lookup, Escape, and IME", async () => {
    const view = open();
    fireEvent.click(screen.getByRole("button", { name: "skills" }));
    fireEvent.change(screen.getByRole("textbox", { name: "searchSkills" }), { target: { value: "review" } });
    fireEvent.click(await screen.findByRole("option", { name: "Review" }));
    expect(actions.getPaneChatSuggestions).toHaveBeenLastCalledWith("task", "pane-a", "skill", "review");
    expect((screen.getByRole("textbox") as HTMLTextAreaElement).value).toBe("$review ");
    edit("/te"); await screen.findByRole("option", { name: "Review" });
    expect(actions.getPaneChatSuggestions).toHaveBeenLastCalledWith("task", "pane-a", "command", "te");
    edit("@src"); await screen.findByRole("option", { name: "Review" });
    expect(actions.getPaneChatSuggestions).toHaveBeenLastCalledWith("task", "pane-a", "file", "src");
    fireEvent.keyDown(screen.getByRole("textbox"), { key: "Enter", isComposing: true });
    expect((screen.getByRole("textbox") as HTMLTextAreaElement).value).toBe("@src");
    fireEvent.keyDown(screen.getByRole("textbox"), { key: "Escape" });
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(view.chat.send).not.toHaveBeenCalled();
  });

  it("ignores stale catalog responses and retries a visible lookup error", async () => {
    let resolve!: (value: PaneChatSuggestions) => void;
    actions.getPaneChatSuggestions.mockReturnValueOnce(new Promise((accept) => { resolve = accept; })).mockRejectedValueOnce(new Error("offline"));
    open(); edit("$r");
    await waitFor(() => expect(actions.getPaneChatSuggestions).toHaveBeenCalledTimes(1));
    edit("$te");
    await screen.findByText("suggestionsError");
    await act(async () => resolve(suggestions()));
    expect(screen.queryByRole("option")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "retryAttachment" }));
    await screen.findByRole("option", { name: "Test" });
  });

  it("debounces rapid queries and blocks Enter while lookup is loading or empty", async () => {
    actions.getPaneChatSuggestions.mockResolvedValue({ targetToken: "pane-a", items: [], truncated: false });
    const view = open(); edit("$r"); edit("$re"); edit("$review");
    fireEvent.keyDown(screen.getByRole("textbox"), { key: "Enter" });
    expect(view.chat.send).not.toHaveBeenCalled();
    await screen.findByText("noSuggestions");
    expect(actions.getPaneChatSuggestions).toHaveBeenCalledTimes(1);
    expect(actions.getPaneChatSuggestions).toHaveBeenCalledWith("task", "pane-a", "skill", "review");
    fireEvent.keyDown(screen.getByRole("textbox"), { key: "Enter" });
    expect(view.chat.send).not.toHaveBeenCalled();
    fireEvent.keyDown(screen.getByRole("textbox"), { key: "Escape" });
    fireEvent.keyDown(screen.getByRole("textbox"), { key: "Enter" });
    await waitFor(() => expect(view.chat.send).toHaveBeenCalledTimes(1));
  });

  it("cleans up an upload whose staging reply arrives after unmount", async () => {
    let resolve!: (value: PaneChatAttachment) => void;
    actions.stagePaneChatAttachment.mockReturnValue(new Promise((accept) => { resolve = accept; }));
    const view = open(); select([image()]);
    await waitFor(() => expect(actions.stagePaneChatAttachment).toHaveBeenCalledTimes(1));
    view.unmount();
    await act(async () => resolve(attachment));
    expect(actions.removePaneChatAttachment).toHaveBeenCalledWith("task", "pane-a", "image-1");
  });

  it("discards abandoned ready attachments from every pane when the composer unmounts", async () => {
    const view = open(); select([image()]);
    await waitFor(() => expect((screen.getByRole("button", { name: "send" }) as HTMLButtonElement).disabled).toBe(false));
    view.switchTo(chat("pane-b"));
    actions.stagePaneChatAttachment.mockResolvedValue({ ...attachment, id: "image-b" });
    select([image()]);
    await waitFor(() => expect((screen.getByRole("button", { name: "send" }) as HTMLButtonElement).disabled).toBe(false));
    view.unmount();
    await waitFor(() => expect(actions.removePaneChatAttachment).toHaveBeenCalledTimes(2));
    expect(actions.removePaneChatAttachment).toHaveBeenCalledWith("task", "pane-a", "image-1");
    expect(actions.removePaneChatAttachment).toHaveBeenCalledWith("task", "pane-b", "image-b");
  });

  it("defers disposal of a captured send until its outcome is known", async () => {
    let resolve!: (accepted: boolean) => void;
    const current = chat();
    vi.mocked(current.send).mockReturnValue(new Promise(accept => { resolve = accept; }));
    const view = open(current); select([image()]);
    await waitFor(() => expect((screen.getByRole("button", { name: "send" }) as HTMLButtonElement).disabled).toBe(false));
    fireEvent.click(screen.getByRole("button", { name: "send" }));
    view.unmount();
    expect(actions.removePaneChatAttachment).not.toHaveBeenCalled();
    await act(async () => resolve(true));
    await waitFor(() => expect(actions.removePaneChatAttachment).toHaveBeenCalledWith("task", "pane-a", "image-1"));
  });

  it("rejects oversized files and excessive file counts without staging bytes", async () => {
    const current = chat(); current.snapshot!.composer!.maxAttachmentBytes = 3;
    open(current); select([image()]);
    await screen.findByText("attachmentTooLarge");
    expect(actions.stagePaneChatAttachment).not.toHaveBeenCalled();
    select(Array.from({ length: 10 }, file));
    expect(screen.getByText("attachmentLimit")).toBeTruthy();
    expect(actions.stagePaneChatAttachment).not.toHaveBeenCalled();
  });
});
