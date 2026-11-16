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
  it("입력창 테두리를 조절하고 작업 중인 API 대화에도 전송할 수 있다", async () => {
    const current = chat();
    current.snapshot!.inputState = "occupied";
    current.snapshot!.composer!.canSendWhileBusy = true;
    const view = open(current);
    expect(screen.queryByRole("button", { name: "expandComposer" })).toBeNull();
    const edge = screen.getByRole("separator", { name: "resizeComposer" });
    fireEvent.keyDown(edge, { key: "ArrowUp" });
    expect((screen.getByRole("textbox") as HTMLTextAreaElement).style.height).toBe("88px");
    fireEvent.keyDown(edge, { key: "Home" });
    expect((screen.getByRole("textbox") as HTMLTextAreaElement).style.height).toBe("64px");
    edit("Continue while working");
    expect((screen.getByRole("button", { name: "send" }) as HTMLButtonElement).disabled).toBe(false);
    fireEvent.click(screen.getByRole("button", { name: "send" }));
    await waitFor(() => expect(view.chat.send).toHaveBeenCalledWith("Continue while working", [], expect.any(String)));
  });
  it("resizes by dragging the border, clamps bounds and stops after release", () => {
    open();
    const edge = screen.getByRole("separator", { name: "resizeComposer" });
    edge.setPointerCapture = vi.fn();
    edge.hasPointerCapture = vi.fn().mockReturnValue(true);
    edge.releasePointerCapture = vi.fn();
    const pointer = (type: string, clientY: number) => {
      const event = new MouseEvent(type, { bubbles: true, button: 0, clientY });
      Object.defineProperty(event, "pointerId", { value: 7 });
      fireEvent(edge, event);
    };
    pointer("pointerdown", 400);
    pointer("pointermove", 250);
    expect((screen.getByRole("textbox") as HTMLTextAreaElement).style.height).toBe("214px");
    pointer("pointermove", 600);
    expect((screen.getByRole("textbox") as HTMLTextAreaElement).style.height).toBe("64px");
    pointer("pointermove", -10000);
    expect(Number.parseInt((screen.getByRole("textbox") as HTMLTextAreaElement).style.height)).toBe(Math.floor(window.innerHeight * 0.6));
    pointer("pointerup", 100);
    pointer("pointermove", 300);
    expect(edge.releasePointerCapture).toHaveBeenCalledWith(7);
    expect(Number.parseInt((screen.getByRole("textbox") as HTMLTextAreaElement).style.height)).toBe(Math.floor(window.innerHeight * 0.6));
  });
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
    const view = open(); edit("Please $e");
    await screen.findByRole("option", { name: "Review" });
    expect(actions.getPaneChatSuggestions).toHaveBeenLastCalledWith("task", "pane-a", "skill", "", 512);
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
    expect(actions.getPaneChatSuggestions).toHaveBeenLastCalledWith("task", "pane-a", "skill", "", 512);
    expect((screen.getByRole("textbox") as HTMLTextAreaElement).value).toBe("$review ");
    edit("/te"); await screen.findByRole("option", { name: "Test" });
    expect(actions.getPaneChatSuggestions).toHaveBeenLastCalledWith("task", "pane-a", "command", "", 512);
    edit("@src"); await screen.findByRole("option", { name: "Review" });
    expect(actions.getPaneChatSuggestions).toHaveBeenLastCalledWith("task", "pane-a", "file", "src");
    fireEvent.keyDown(screen.getByRole("textbox"), { key: "Enter", isComposing: true });
    expect((screen.getByRole("textbox") as HTMLTextAreaElement).value).toBe("@src");
    fireEvent.keyDown(screen.getByRole("textbox"), { key: "Escape" });
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(view.chat.send).not.toHaveBeenCalled();
  });

  it("ignores stale file responses and retries a visible lookup error", async () => {
    let resolve!: (value: PaneChatSuggestions) => void;
    actions.getPaneChatSuggestions.mockReturnValueOnce(new Promise((accept) => { resolve = accept; })).mockRejectedValueOnce(new Error("offline"));
    open(); edit("@r");
    await waitFor(() => expect(actions.getPaneChatSuggestions).toHaveBeenCalledTimes(1));
    edit("@te");
    await screen.findByText("suggestionsError");
    await act(async () => resolve(suggestions()));
    expect(screen.queryByRole("option")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "retryAttachment" }));
    await screen.findByRole("option", { name: "Test" });
  });

  it("requests the skill list once for rapid typing and blocks Enter while it is loading or empty", async () => {
    actions.getPaneChatSuggestions.mockResolvedValue({ targetToken: "pane-a", items: [], truncated: false });
    const view = open(); edit("$r"); edit("$re"); edit("$review");
    fireEvent.keyDown(screen.getByRole("textbox"), { key: "Enter" });
    expect(view.chat.send).not.toHaveBeenCalled();
    await screen.findByText("noSuggestions");
    expect(actions.getPaneChatSuggestions).toHaveBeenCalledTimes(1);
    expect(actions.getPaneChatSuggestions).toHaveBeenCalledWith("task", "pane-a", "skill", "", 512);
    fireEvent.keyDown(screen.getByRole("textbox"), { key: "Enter" });
    expect(view.chat.send).not.toHaveBeenCalled();
    fireEvent.keyDown(screen.getByRole("textbox"), { key: "Escape" });
    fireEvent.keyDown(screen.getByRole("textbox"), { key: "Enter" });
    await waitFor(() => expect(view.chat.send).toHaveBeenCalledTimes(1));
  });

  it("filters a loaded command list on screen for each keystroke without asking the desktop again", async () => {
    open(); edit("/");
    await screen.findByRole("option", { name: "Review" });
    expect(actions.getPaneChatSuggestions).toHaveBeenCalledTimes(1);
    edit("/te");
    expect(screen.getAllByRole("option").map((option) => option.getAttribute("aria-label"))).toEqual(["Test"]);
    edit("/tes");
    expect(screen.getAllByRole("option").map((option) => option.getAttribute("aria-label"))).toEqual(["Test"]);
    edit("/zzz");
    expect(screen.queryByRole("option")).toBeNull();
    expect(screen.getByText("noSuggestions")).toBeTruthy();
    expect(screen.queryByText("loadingSuggestions")).toBeNull();
    expect(actions.getPaneChatSuggestions).toHaveBeenCalledTimes(1);
  });

  it("asks the desktop for each query when the skill list arrives incomplete", async () => {
    actions.getPaneChatSuggestions.mockImplementation(async (_taskId: string, targetToken: string, _kind: string, query: string, limit?: number) => limit
      ? { targetToken, truncated: true, items: [{ id: "skill-0", label: "Alpha", description: "First of many", insertText: "$alpha" }] }
      : { targetToken, truncated: false, items: [{ id: "skill-found", label: "Found " + query, description: "From desktop", insertText: "$found" }] });
    open(); edit("$");
    await screen.findByRole("option", { name: "Found" });
    edit("$rev");
    await screen.findByRole("option", { name: "Found rev" });
    expect(actions.getPaneChatSuggestions).toHaveBeenLastCalledWith("task", "pane-a", "skill", "rev");
    expect(screen.queryByRole("option", { name: "Alpha" })).toBeNull();
  });

  it("shows the remembered skill list at once when the menu reopens and refreshes it in the background", async () => {
    open(); edit("$");
    await screen.findByRole("option", { name: "Review" });
    edit("");
    expect(screen.queryByRole("listbox")).toBeNull();
    let refresh!: (value: PaneChatSuggestions) => void;
    actions.getPaneChatSuggestions.mockReturnValueOnce(new Promise((accept) => { refresh = accept; }));
    edit("$");
    expect(screen.getAllByRole("option").map((option) => option.getAttribute("aria-label"))).toEqual(["Review", "Test"]);
    expect(actions.getPaneChatSuggestions).toHaveBeenCalledTimes(2);
    await act(async () => refresh({ targetToken: "pane-a", truncated: false, items: [...suggestions().items, { id: "skill-3", label: "Plan", description: "Outline work", insertText: "$plan" }] }));
    expect(screen.getAllByRole("option").map((option) => option.getAttribute("aria-label"))).toEqual(["Plan", "Review", "Test"]);
  });

  it("keeps the remembered skill list when a background refresh fails", async () => {
    open(); edit("$");
    await screen.findByRole("option", { name: "Review" });
    edit("");
    actions.getPaneChatSuggestions.mockRejectedValueOnce(new Error("offline"));
    edit("$");
    await waitFor(() => expect(actions.getPaneChatSuggestions).toHaveBeenCalledTimes(2));
    await act(async () => {});
    expect(screen.getAllByRole("option").map((option) => option.getAttribute("aria-label"))).toEqual(["Review", "Test"]);
    expect(screen.queryByText("suggestionsError")).toBeNull();
  });

  it("shows a skill list error, retries it, and ignores a list answered for another pane", async () => {
    actions.getPaneChatSuggestions.mockRejectedValueOnce(new Error("offline")).mockResolvedValueOnce(suggestions("pane-b"));
    open(); edit("$");
    await screen.findByText("suggestionsError");
    fireEvent.click(screen.getByRole("button", { name: "retryAttachment" }));
    await waitFor(() => expect(actions.getPaneChatSuggestions).toHaveBeenCalledTimes(2));
    await screen.findByText("suggestionsError");
    expect(screen.queryByRole("option")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "retryAttachment" }));
    await screen.findByRole("option", { name: "Review" });
  });

  it("waits for typing to pause before asking the desktop for matching files", async () => {
    open(); edit("@s"); edit("@sr"); edit("@src");
    await screen.findByRole("option", { name: "Review" });
    expect(actions.getPaneChatSuggestions).toHaveBeenCalledTimes(1);
    expect(actions.getPaneChatSuggestions).toHaveBeenCalledWith("task", "pane-a", "file", "src");
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

it("preserves a conversation draft after lease renewal and requires attachments to be restaged", async () => {
  const first = chat("execution-a");
  Object.assign(first.snapshot!, { conversationKey: "scoped-conversation-a" });
  const view = open(first);
  edit("Keep my draft after reconnect");
  select([file()]);
  await waitFor(() => expect(actions.stagePaneChatAttachment).toHaveBeenCalledTimes(1));
  await screen.findByText("notes.txt");
  const renewed = chat("execution-b");
  Object.assign(renewed.snapshot!, { conversationKey: "scoped-conversation-a" });
  view.switchTo(renewed);
  expect((screen.getByRole("textbox") as HTMLTextAreaElement).value).toBe("Keep my draft after reconnect");
  await screen.findByRole("button", { name: "retryAttachment notes.txt" });
  expect((screen.getByRole("button", { name: "send" }) as HTMLButtonElement).disabled).toBe(true);
  fireEvent.click(screen.getByRole("button", { name: "retryAttachment notes.txt" }));
  await waitFor(() => expect(actions.stagePaneChatAttachment).toHaveBeenLastCalledWith("task", "execution-b", expect.objectContaining({ name: "notes.txt" })));
});

it("preserves an unresolved operation ID when the same conversation renews its token and restages files", async () => {
  const first = chat("old-token");
  Object.assign(first.snapshot!, { conversationKey: "uncertain-conversation" });
  vi.mocked(first.send).mockResolvedValue(false);
  actions.stagePaneChatAttachment.mockResolvedValueOnce({ ...attachment, id: "original-file", name: "notes.txt", kind: "file" });
  const view = open(first); edit("Unknown result"); select([file()]);
  await waitFor(() => expect((screen.getByRole("button", { name: "send" }) as HTMLButtonElement).disabled).toBe(false));
  fireEvent.click(screen.getByRole("button", { name: "send" })); await screen.findByText("sendError");
  const requestId = vi.mocked(first.send).mock.calls[0][2];
  const renewed = chat("new-token"); Object.assign(renewed.snapshot!, { conversationKey: "uncertain-conversation" });
  vi.mocked(renewed.send).mockResolvedValue(false);
  actions.stagePaneChatAttachment.mockResolvedValueOnce({ ...attachment, id: "restaged-file", name: "notes.txt", kind: "file" });
  view.switchTo(renewed);
  fireEvent.click(await screen.findByRole("button", { name: "retryAttachment notes.txt" }));
  await waitFor(() => expect((screen.getByRole("button", { name: "send" }) as HTMLButtonElement).disabled).toBe(false));
  fireEvent.click(screen.getByRole("button", { name: "send" }));
  await waitFor(() => expect(renewed.send).toHaveBeenCalledWith("Unknown result", ["restaged-file"], requestId));
});


it("ignores an old upload failure arriving after the renewed target restaged the file", async () => {
  let rejectOld!: (error: Error) => void;
  actions.stagePaneChatAttachment.mockReturnValueOnce(new Promise((_resolve, reject) => { rejectOld = reject; }));
  const first = chat("old-execution");
  Object.assign(first.snapshot!, { conversationKey: "same-scoped-conversation" });
  const view = open(first);
  edit("Retain this draft");
  select([file()]);
  await waitFor(() => expect(actions.stagePaneChatAttachment).toHaveBeenCalledTimes(1));
  const renewed = chat("renewed-execution");
  Object.assign(renewed.snapshot!, { conversationKey: "same-scoped-conversation" });
  view.switchTo(renewed);
  fireEvent.click(await screen.findByRole("button", { name: "retryAttachment notes.txt" }));
  await waitFor(() => expect((screen.getByRole("button", { name: "send" }) as HTMLButtonElement).disabled).toBe(false));
  await act(async () => rejectOld(new Error("Old execution upload failed")));
  expect(screen.queryByText("Old execution upload failed")).toBeNull();
  expect((screen.getByRole("button", { name: "send" }) as HTMLButtonElement).disabled).toBe(false);
});
