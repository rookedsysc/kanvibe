import { useState } from "react";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import DiffPageClient from "../DiffPageClient";
import type { DiffFile } from "@/desktop/renderer/actions/diff";

const { original, modified, save } = vi.hoisted(() => ({ original: vi.fn(), modified: vi.fn(), save: vi.fn() }));
vi.mock("next-intl", () => ({ useTranslations: () => (key: string) => key }));
vi.mock("@/desktop/renderer/actions/diff", () => ({ getOriginalFileContent: original, getFileContent: modified, saveFileContent: save }));
vi.mock("@/components/DiffFileTree", () => ({ default: ({ onSelectFile }: { onSelectFile: (path: string) => void }) => <button onClick={() => onSelectFile("B")}>select B</button> }));
vi.mock("@/components/DiffMonacoViewer", () => ({ default: ({ filePath, originalContent, modifiedContent }: { filePath: string; originalContent: string; modifiedContent: string }) => <div data-testid="viewer" data-original={originalContent}>{filePath}:{modifiedContent}</div> }));
vi.mock("@/components/DiffFileEditor", () => ({ default: ({ onSave, content }: { onSave: (content: string) => Promise<void>; content: string }) => {
  const [draft, setDraft] = useState(content);
  return <><input aria-label="draft" value={draft} onChange={(event) => setDraft(event.target.value)} /><button onClick={() => { void onSave("edited"); }}>save {content}</button></>;
} }));
const files: DiffFile[] = ["A", "B"].map((path) => ({ path, status: "modified", additions: 1, deletions: 0 }));
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej; });
  return { promise, resolve, reject };
}
afterEach(cleanup);
beforeEach(() => { vi.resetAllMocks(); original.mockResolvedValue("original"); modified.mockResolvedValue("modified"); save.mockResolvedValue({ success: true }); });

describe("DiffPageClient selection ownership", () => {
  it("saves the displayed default file without a file-tree click", async () => {
    render(<DiffPageClient taskId="task" files={files} />);
    await screen.findByTestId("viewer");
    fireEvent.click(screen.getByText("editMode"));
    fireEvent.click(screen.getByText("save modified"));
    await waitFor(() => expect(save).toHaveBeenCalledWith("task", "A", "edited"));
  });

  it("reads each file once and keeps the current file when an older response finishes last", async () => {
    const firstOriginal = deferred<string>();
    const firstModified = deferred<string>();
    original.mockImplementation((_task, path) => path === "A" ? firstOriginal.promise : Promise.resolve("B-original"));
    modified.mockImplementation((_task, path) => path === "A" ? firstModified.promise : Promise.resolve("B-modified"));
    render(<DiffPageClient taskId="task" files={files} />);
    fireEvent.click(screen.getByText("select B"));
    await waitFor(() => expect(screen.getByTestId("viewer").textContent).toBe("B:B-modified"));
    await act(async () => { firstOriginal.resolve("A-original"); firstModified.resolve("A-modified"); });
    expect(screen.getByTestId("viewer").textContent).toBe("B:B-modified");
    expect(original.mock.calls.filter(([, path]) => path === "B")).toHaveLength(1);
    expect(modified.mock.calls.filter(([, path]) => path === "B")).toHaveLength(1);
  });

  it("ends loading on read failure without exposing a writable stale editor", async () => {
    modified.mockRejectedValue(new Error("read unavailable"));
    render(<DiffPageClient taskId="task" files={files} />);
    expect(await screen.findByRole("alert")).toHaveProperty("textContent", "read unavailable");
    fireEvent.click(screen.getByText("editMode"));
    expect(screen.queryByText(/^save /)).toBeNull();
    expect(save).not.toHaveBeenCalled();
  });

  it("ignores responses from a previous task even with the same file name", async () => {
    const pending = deferred<string>();
    modified.mockImplementation((task) => task === "first" ? pending.promise : Promise.resolve("second-content"));
    const { rerender } = render(<DiffPageClient taskId="first" files={files} />);
    rerender(<DiffPageClient taskId="second" files={files} />);
    await waitFor(() => expect(screen.getByTestId("viewer").textContent).toBe("A:second-content"));
    await act(async () => { pending.resolve("first-content"); });
    expect(screen.getByTestId("viewer").textContent).toBe("A:second-content");
  });
});

describe("DiffPageClient file inventory refresh", () => {
  it("reloads selected content when a new inventory arrives with the same file path", async () => {
    modified.mockResolvedValueOnce("before").mockResolvedValueOnce("after");
    const { rerender } = render(<DiffPageClient taskId="task" files={files} />);
    await waitFor(() => expect(screen.getByTestId("viewer").textContent).toBe("A:before"));
    rerender(<DiffPageClient taskId="task" files={files.map((file) => ({ ...file, additions: 10 }))} />);
    await waitFor(() => expect(screen.getByTestId("viewer").textContent).toBe("A:after"));
    expect(modified).toHaveBeenCalledTimes(2);
  });

  it("keeps an editor draft mounted during a refresh and shows fresh content in diff mode", async () => {
    const refresh = deferred<string>();
    modified.mockResolvedValueOnce("before").mockReturnValueOnce(refresh.promise);
    const { rerender } = render(<DiffPageClient taskId="task" files={files} />);
    await screen.findByTestId("viewer");
    fireEvent.click(screen.getByText("editMode"));
    fireEvent.change(screen.getByLabelText("draft"), { target: { value: "unsaved draft" } });
    rerender(<DiffPageClient taskId="task" files={[...files]} />);
    expect((screen.getByLabelText("draft") as HTMLInputElement).value).toBe("unsaved draft");
    await act(async () => refresh.resolve("after"));
    expect((screen.getByLabelText("draft") as HTMLInputElement).value).toBe("unsaved draft");
    fireEvent.click(screen.getByText("diffMode"));
    expect(screen.getByTestId("viewer").textContent).toBe("A:after");
  });

  it("preserves an editor draft when a same-file refresh fails", async () => {
    modified.mockResolvedValueOnce("before").mockRejectedValueOnce(new Error("refresh unavailable"));
    const { rerender } = render(<DiffPageClient taskId="task" files={files} />);
    await screen.findByTestId("viewer");
    fireEvent.click(screen.getByText("editMode"));
    fireEvent.change(screen.getByLabelText("draft"), { target: { value: "unsaved draft" } });
    rerender(<DiffPageClient taskId="task" files={[...files]} />);
    await waitFor(() => expect(modified).toHaveBeenCalledTimes(2));
    expect((screen.getByLabelText("draft") as HTMLInputElement).value).toBe("unsaved draft");
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("selects a remaining file when the explicit selection disappears", async () => {
    modified.mockImplementation((_task, path) => Promise.resolve(path + "-content"));
    const { rerender } = render(<DiffPageClient taskId="task" files={files} />);
    fireEvent.click(screen.getByText("select B"));
    await waitFor(() => expect(screen.getByTestId("viewer").textContent).toBe("B:B-content"));
    rerender(<DiffPageClient taskId="task" files={[files[0]]} />);
    await waitFor(() => expect(screen.getByTestId("viewer").textContent).toBe("A:A-content"));
    rerender(<DiffPageClient taskId="task" files={[]} />);
    expect(screen.queryByTestId("viewer")).toBeNull();
  });
});


describe("DiffPageClient rename original", () => {
  it("uses the previous path for the original and the destination for reading and saving", async () => {
    original.mockImplementation((_task, path) => Promise.resolve(path === "old.txt" ? "base content" : ""));
    render(<DiffPageClient taskId="task" files={[{ path: "new.txt", originalPath: "old.txt", status: "renamed", additions: 0, deletions: 0 }]} />);
    expect((await screen.findByTestId("viewer")).getAttribute("data-original")).toBe("base content");
    expect(modified).toHaveBeenCalledWith("task", "new.txt");
    fireEvent.click(screen.getByText("editMode"));
    fireEvent.click(screen.getByText("save modified"));
    await waitFor(() => expect(save).toHaveBeenCalledWith("task", "new.txt", "edited"));
  });
});
