import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import DiffPageClient from "@/components/DiffPageClient";
import type { DiffFile } from "@/app/actions/diff";

// --- Mocks ---

const mockGetOriginalFileContent = vi.fn();
const mockGetFileContent = vi.fn();
const mockSaveFileContent = vi.fn();

vi.mock("@/app/actions/diff", () => ({
  getOriginalFileContent: (...args: unknown[]) => mockGetOriginalFileContent(...args),
  getFileContent: (...args: unknown[]) => mockGetFileContent(...args),
  saveFileContent: (...args: unknown[]) => mockSaveFileContent(...args),
}));

vi.mock("next-intl", () => ({
  useTranslations: () => (key: string) => key,
}));

/** Monaco 에디터는 dynamic import로 로딩되므로 mock 처리한다 */
vi.mock("next/dynamic", () => ({
  default: () => {
    const MockComponent = () => <div data-testid="mock-editor">Editor</div>;
    MockComponent.displayName = "MockDynamic";
    return MockComponent;
  },
}));

const createFile = (
  path: string,
  status: DiffFile["status"] = "modified",
  additions = 5,
  deletions = 2
): DiffFile => ({
  path,
  status,
  additions,
  deletions,
});

describe("DiffPageClient", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockGetOriginalFileContent.mockResolvedValue("original content");
    mockGetFileContent.mockResolvedValue("modified content");
  });

  it("should auto-select first file on mount", async () => {
    // Given
    const files = [createFile("src/index.ts"), createFile("src/app.ts")];

    // When
    render(<DiffPageClient taskId="task-1" files={files} />);

    // Then
    await waitFor(() => {
      expect(mockGetOriginalFileContent).toHaveBeenCalledWith("task-1", "src/index.ts");
      expect(mockGetFileContent).toHaveBeenCalledWith("task-1", "src/index.ts");
    });
  });

  it("should display file path in header after selection", async () => {
    // Given
    const files = [createFile("src/index.ts")];

    // When
    render(<DiffPageClient taskId="task-1" files={files} />);

    // Then
    await waitFor(() => {
      expect(screen.getByText("src/index.ts")).toBeDefined();
    });
  });

  it("should toggle viewed state when clicking checkbox", async () => {
    // Given
    const files = [createFile("src/index.ts")];

    // When
    render(<DiffPageClient taskId="task-1" files={files} />);

    await waitFor(() => {
      expect(screen.getByText("src/index.ts")).toBeDefined();
    });

    const checkbox = screen.getByRole("checkbox") as HTMLInputElement;
    expect(checkbox.checked).toBe(false);

    // When
    fireEvent.click(checkbox);

    // Then
    expect(checkbox.checked).toBe(true);
  });

  it("should change viewed label text based on state", async () => {
    // Given
    const files = [createFile("src/index.ts")];

    // When
    render(<DiffPageClient taskId="task-1" files={files} />);

    await waitFor(() => {
      expect(screen.getByText("markViewed")).toBeDefined();
    });

    // When
    fireEvent.click(screen.getByRole("checkbox"));

    // Then
    expect(screen.getByText("viewed")).toBeDefined();
  });

  it("should show empty state when no files", () => {
    // Given / When
    render(<DiffPageClient taskId="task-1" files={[]} />);

    // Then
    expect(screen.getAllByText("noChanges").length).toBeGreaterThanOrEqual(1);
  });

  it("should display viewed progress counter", async () => {
    // Given
    const files = [createFile("a.ts"), createFile("b.ts"), createFile("c.ts")];

    // When
    render(<DiffPageClient taskId="task-1" files={files} />);

    // Then
    await waitFor(() => {
      expect(screen.getByText("0 / 3")).toBeDefined();
    });

    // When - viewed 체크
    const checkbox = screen.getByRole("checkbox");
    fireEvent.click(checkbox);

    // Then
    expect(screen.getByText("1 / 3")).toBeDefined();
  });

  it("should show change stats for selected file", async () => {
    // Given
    const files = [createFile("src/index.ts", "modified", 10, 3)];

    // When
    render(<DiffPageClient taskId="task-1" files={files} />);

    // Then - 사이드바와 헤더에 각각 표시되므로 getAllByText를 사용한다
    await waitFor(() => {
      expect(screen.getAllByText("+10").length).toBeGreaterThanOrEqual(1);
      expect(screen.getAllByText("-3").length).toBeGreaterThanOrEqual(1);
    });
  });
});
