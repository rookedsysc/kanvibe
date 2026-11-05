import { describe, it, expect, vi, beforeEach } from "vitest";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import FolderSearchInput from "../FolderSearchInput";

const mockListSubdirectories = vi.fn();

vi.mock("next-intl", () => ({
  useTranslations: () => (key: string, values?: Record<string, unknown>) => {
    if (key === "folderCount") {
      return `count:${values?.count}`;
    }

    return key;
  },
}));

vi.mock("@/desktop/renderer/actions/project", () => ({
  listSubdirectories: (...args: unknown[]) => mockListSubdirectories(...args),
}));

vi.mock("@/components/HighlightedText", () => ({
  default: ({ text }: { text: string }) => <span>{text}</span>,
}));

describe("FolderSearchInput", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockListSubdirectories.mockResolvedValue(["api", "web"]);
  });

  it("디렉터리 조회가 동기적으로 실패해도 입력을 계속 사용할 수 있다", async () => {
    mockListSubdirectories.mockImplementation(() => { throw new Error("IPC unavailable"); });
    render(<FolderSearchInput name="scanPath" onSelect={vi.fn()} />);
    const textbox = screen.getByRole("textbox");
    fireEvent.focus(textbox);
    fireEvent.change(textbox, { target: { value: "/manual/path" } });
    await waitFor(() => {
      expect(document.querySelector('input[type="hidden"][name="scanPath"]')).toHaveProperty("value", "/manual/path");
      expect(screen.queryByRole("button")).toBeNull();
    });
  });

  it("직접 입력한 원격 경로를 hidden input으로 제출한다", async () => {
    // Given
    render(<FolderSearchInput name="scanPath" sshHost="remote-host" onSelect={vi.fn()} />);

    // When
    const textbox = screen.getByRole("textbox");
    fireEvent.change(textbox, { target: { value: "~/workspace" } });

    // Then
    await waitFor(() => {
      expect(mockListSubdirectories).toHaveBeenCalledWith("~", "remote-host");
    });

    const hiddenInput = document.querySelector('input[type="hidden"][name="scanPath"]') as HTMLInputElement;
    expect(hiddenInput.value).toBe("~/workspace");
  });

  it("목록에서 선택한 경로가 있으면 선택 경로를 우선 제출한다", async () => {
    // Given
    const onSelect = vi.fn();
    render(<FolderSearchInput name="scanPath" sshHost="remote-host" onSelect={onSelect} />);

    // When
    const textbox = screen.getByRole("textbox");
    fireEvent.focus(textbox);

    const option = await screen.findByRole("button", { name: "api" });
    fireEvent.click(option);

    // Then
    const hiddenInput = document.querySelector('input[type="hidden"][name="scanPath"]') as HTMLInputElement;
    expect(hiddenInput.value).toBe("~/api");
    expect(onSelect).toHaveBeenCalledWith("~/api");
  });

  it("이전 경로의 늦은 응답과 로딩 중 키보드 선택을 무시한다", async () => {
    let resolveOld!: (value: string[]) => void;
    let resolveNew!: (value: string[]) => void;
    mockListSubdirectories.mockImplementation((path: string) => {
      if (path === "/old") return new Promise<string[]>((resolve) => { resolveOld = resolve; });
      if (path === "/new") return new Promise<string[]>((resolve) => { resolveNew = resolve; });
      return Promise.resolve(["home-child"]);
    });
    const onSelect = vi.fn();
    render(<FolderSearchInput name="scanPath" onSelect={onSelect} />);
    const textbox = screen.getByRole("textbox");
    fireEvent.focus(textbox);
    await screen.findByRole("button", { name: "home-child" });
    fireEvent.change(textbox, { target: { value: "/old/" } });
    fireEvent.keyDown(textbox, { key: "Enter" });
    expect(onSelect).not.toHaveBeenCalled();
    fireEvent.change(textbox, { target: { value: "/new/" } });
    await act(async () => { resolveNew(["new-child"]); });
    await screen.findByRole("button", { name: "new-child" });
    await act(async () => { resolveOld(["old-child"]); });
    expect(screen.queryByRole("button", { name: "old-child" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "new-child" }));
    expect(onSelect).toHaveBeenCalledWith("/new/new-child");
  });

  it("호스트 변경 뒤 이전 호스트의 실패가 현재 목록을 지우지 않는다", async () => {
    let rejectOld!: (error: Error) => void;
    mockListSubdirectories.mockImplementation((_path: string, host: string) => host === "old-host"
      ? new Promise<string[]>((_resolve, reject) => { rejectOld = reject; })
      : Promise.resolve(["new-host-child"]));
    const onSelect = vi.fn();
    const { rerender } = render(<FolderSearchInput name="scanPath" sshHost="old-host" onSelect={onSelect} />);
    fireEvent.focus(screen.getByRole("textbox"));
    rerender(<FolderSearchInput name="scanPath" sshHost="new-host" onSelect={onSelect} />);
    await screen.findByRole("button", { name: "new-host-child" });
    await act(async () => { rejectOld(new Error("old host unavailable")); });
    fireEvent.click(screen.getByRole("button", { name: "new-host-child" }));
    expect(onSelect).toHaveBeenCalledWith("~/new-host-child");
  });

  it("호스트 변경 중 이전 성공과 finally가 현재 로딩을 끝내지 않는다", async () => {
    let resolveOld!: (value: string[]) => void;
    let resolveNew!: (value: string[]) => void;
    mockListSubdirectories.mockImplementation((_path: string, host: string) => new Promise<string[]>((resolve) => {
      if (host === "old-host") resolveOld = resolve;
      else resolveNew = resolve;
    }));
    const { rerender } = render(<FolderSearchInput name="scanPath" sshHost="old-host" onSelect={vi.fn()} />);
    fireEvent.focus(screen.getByRole("textbox"));
    rerender(<FolderSearchInput name="scanPath" sshHost="new-host" onSelect={vi.fn()} />);
    await act(async () => { resolveOld(["old-host-child"]); });
    expect(screen.getByText("loadingFolders")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "old-host-child" })).toBeNull();
    await act(async () => { resolveNew(["new-host-child"]); });
    expect(screen.getByRole("button", { name: "new-host-child" })).toBeTruthy();
  });

  it.each([undefined, "remote-host"])("절대 루트의 클릭과 Tab 경로를 유지한다 (%s)", async (sshHost) => {
    mockListSubdirectories.mockResolvedValue(["prefix"]);
    const onSelect = vi.fn();
    render(<FolderSearchInput name="scanPath" sshHost={sshHost} onSelect={onSelect} />);
    const textbox = screen.getByRole("textbox");
    fireEvent.change(textbox, { target: { value: "/" } });
    await screen.findByRole("button", { name: "prefix" });
    expect(mockListSubdirectories).toHaveBeenLastCalledWith("/", sshHost);
    fireEvent.click(screen.getByRole("button", { name: "prefix" }));
    expect(onSelect).toHaveBeenLastCalledWith("/prefix");
    fireEvent.change(textbox, { target: { value: "/pre" } });
    await screen.findByRole("button", { name: "prefix" });
    fireEvent.keyDown(textbox, { key: "Tab" });
    expect((textbox as HTMLInputElement).value).toBe("/prefix/");
    await waitFor(() => { expect(mockListSubdirectories).toHaveBeenLastCalledWith("/prefix", sshHost); });
  });

});
