import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import DiffFileEditor from "../DiffFileEditor";

const editorState = vi.hoisted(() => ({
  value: "original",
  saveShortcut: null as (() => void) | null,
}));

vi.mock("@monaco-editor/react", () => ({
  default: ({
    onMount,
    onChange,
  }: {
    onMount: (editor: unknown) => void;
    onChange?: (value: string | undefined) => void;
  }) => {
    onMount({
      getValue: () => editorState.value,
      addCommand: (_keybinding: number, handler: () => void) => {
        editorState.saveShortcut = handler;
      },
    });
    return (
      <textarea
        aria-label="editor"
        onChange={(event) => {
          editorState.value = event.target.value;
          onChange?.(event.target.value);
        }}
      />
    );
  },
}));

vi.mock("../DiffMonacoViewer", () => ({
  detectLanguage: () => "typescript",
}));

const labels = {
  save: "save",
  saving: "saving",
  saved: "saved",
  saveError: "saveError",
  unsavedChanges: "unsavedChanges",
};

describe("DiffFileEditor", () => {
  afterEach(() => {
    vi.useRealTimers();
    editorState.value = "original";
    editorState.saveShortcut = null;
  });

  it("저장 중에는 버튼과 단축키 어느 쪽으로 다시 저장해도 저장을 한 번만 보낸다", async () => {
    let resolveSave: () => void = () => {};
    const onSave = vi.fn(() => new Promise<void>((resolve) => {
      resolveSave = resolve;
    }));
    render(<DiffFileEditor content="original" filePath="src/a.ts" onSave={onSave} labels={labels} />);

    fireEvent.click(screen.getByRole("button", { name: "save" }));
    act(() => {
      editorState.saveShortcut?.();
      editorState.saveShortcut?.();
    });

    expect(onSave).toHaveBeenCalledTimes(1);

    resolveSave();
    await screen.findByRole("button", { name: "saved" });
  });

  it("저장이 실패하면 사유를 시간이 지나도 남기고 다시 저장으로 재시도할 수 있다", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const onSave = vi.fn()
      .mockRejectedValueOnce(new Error("EACCES: permission denied"))
      .mockResolvedValueOnce(undefined);
    render(<DiffFileEditor content="original" filePath="src/a.ts" onSave={onSave} labels={labels} />);

    fireEvent.click(screen.getByRole("button", { name: "save" }));

    await waitFor(() => {
      expect(screen.getByRole("alert").textContent).toContain("EACCES: permission denied");
    });
    act(() => {
      vi.advanceTimersByTime(10_000);
    });
    expect(screen.getByRole("alert").textContent).toContain("EACCES: permission denied");

    fireEvent.click(screen.getByRole("button", { name: "save" }));

    await waitFor(() => {
      expect(screen.queryByRole("alert")).toBeNull();
    });
    expect(onSave).toHaveBeenCalledTimes(2);
  });

  it("편집하면 저장하지 않은 변경을 표시하고 저장이 끝나면 지운다", async () => {
    const onSave = vi.fn().mockResolvedValue(undefined);
    render(<DiffFileEditor content="original" filePath="src/a.ts" onSave={onSave} labels={labels} />);

    expect(screen.queryByText("unsavedChanges")).toBeNull();

    fireEvent.change(screen.getByLabelText("editor"), { target: { value: "edited" } });
    expect(screen.getByText("unsavedChanges")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "save" }));

    await waitFor(() => {
      expect(screen.queryByText("unsavedChanges")).toBeNull();
    });
    expect(onSave).toHaveBeenCalledWith("edited");
  });
});
