import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import DiffFileTree from "@/components/DiffFileTree";
import type { DiffFile } from "@/app/actions/diff";

vi.mock("next-intl", () => ({
  useTranslations: () => (key: string) => key,
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

describe("DiffFileTree", () => {
  it("should render file names in tree structure", () => {
    // Given
    const files = [
      createFile("src/index.ts"),
      createFile("src/utils/helper.ts", "added"),
    ];

    // When
    render(
      <DiffFileTree
        files={files}
        selectedFile={null}
        onSelectFile={vi.fn()}
      />
    );

    // Then
    expect(screen.getByText("index.ts")).toBeDefined();
    expect(screen.getByText("helper.ts")).toBeDefined();
    expect(screen.getByText("src")).toBeDefined();
  });

  it("should call onSelectFile when clicking a file", () => {
    // Given
    const files = [createFile("README.md", "added")];
    const onSelectFile = vi.fn();

    // When
    render(
      <DiffFileTree
        files={files}
        selectedFile={null}
        onSelectFile={onSelectFile}
      />
    );
    fireEvent.click(screen.getByText("README.md"));

    // Then
    expect(onSelectFile).toHaveBeenCalledWith("README.md");
  });

  it("should highlight selected file", () => {
    // Given
    const files = [createFile("src/index.ts"), createFile("src/app.ts")];

    // When
    render(
      <DiffFileTree
        files={files}
        selectedFile="src/index.ts"
        onSelectFile={vi.fn()}
      />
    );

    // Then
    const selectedButton = screen.getByText("index.ts").closest("button");
    expect(selectedButton?.className).toContain("bg-brand-subtle");
  });

  it("should display status labels (A, M, D, R)", () => {
    // Given
    const files = [
      createFile("added.ts", "added"),
      createFile("modified.ts", "modified"),
      createFile("deleted.ts", "deleted"),
      createFile("renamed.ts", "renamed"),
    ];

    // When
    render(
      <DiffFileTree
        files={files}
        selectedFile={null}
        onSelectFile={vi.fn()}
      />
    );

    // Then
    expect(screen.getByText("A")).toBeDefined();
    expect(screen.getByText("M")).toBeDefined();
    expect(screen.getByText("D")).toBeDefined();
    expect(screen.getByText("R")).toBeDefined();
  });

  it("should show check icon for viewed files", () => {
    // Given
    const files = [createFile("src/viewed.ts"), createFile("src/unviewed.ts")];
    const viewedFiles = new Set(["src/viewed.ts"]);

    // When
    render(
      <DiffFileTree
        files={files}
        selectedFile={null}
        onSelectFile={vi.fn()}
        viewedFiles={viewedFiles}
      />
    );

    // Then
    const viewedButton = screen.getByText("viewed.ts").closest("button");
    const unviewedButton = screen.getByText("unviewed.ts").closest("button");

    /** viewed 파일은 muted 처리된다 */
    expect(viewedButton?.className).toContain("text-text-muted");
    /** unviewed 파일은 기본 색상이다 */
    expect(unviewedButton?.className).toContain("text-text-secondary");
  });

  it("should apply strikethrough to viewed file names", () => {
    // Given
    const files = [createFile("src/viewed.ts")];
    const viewedFiles = new Set(["src/viewed.ts"]);

    // When
    render(
      <DiffFileTree
        files={files}
        selectedFile={null}
        onSelectFile={vi.fn()}
        viewedFiles={viewedFiles}
      />
    );

    // Then
    const nameSpan = screen.getByText("viewed.ts");
    expect(nameSpan.className).toContain("line-through");
  });

  it("should toggle folder expansion on click", () => {
    // Given
    const files = [createFile("src/deep/file.ts")];

    // When
    render(
      <DiffFileTree
        files={files}
        selectedFile={null}
        onSelectFile={vi.fn()}
      />
    );

    /** 폴더는 기본적으로 펼쳐져 있다 */
    expect(screen.getByText("file.ts")).toBeDefined();

    /** src 폴더를 클릭하여 접는다 */
    fireEvent.click(screen.getByText("src"));

    /** 접힌 후 deep 폴더와 파일이 숨겨진다 */
    expect(screen.queryByText("file.ts")).toBeNull();
  });

  it("should build nested tree from flat file paths", () => {
    // Given
    const files = [
      createFile("src/components/Button.tsx", "added"),
      createFile("src/components/Modal.tsx", "modified"),
      createFile("README.md", "modified"),
    ];

    // When
    render(
      <DiffFileTree
        files={files}
        selectedFile={null}
        onSelectFile={vi.fn()}
      />
    );

    // Then
    expect(screen.getByText("src")).toBeDefined();
    expect(screen.getByText("components")).toBeDefined();
    expect(screen.getByText("Button.tsx")).toBeDefined();
    expect(screen.getByText("Modal.tsx")).toBeDefined();
    expect(screen.getByText("README.md")).toBeDefined();
  });
});
