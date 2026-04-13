import { describe, it, expect, vi, beforeEach } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
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
});
