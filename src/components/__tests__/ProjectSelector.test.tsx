import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, cleanup } from "@testing-library/react";
import type { Project } from "@/entities/Project";

vi.mock("next-intl", () => ({
  useTranslations: () => (key: string) => key,
}));

import ProjectSelector from "../ProjectSelector";

function createProject(id: string, name: string): Project {
  return {
    id,
    name,
    repoPath: `/repo/${id}`,
    defaultBranch: "main",
    sshHost: null,
    isWorktree: false,
    color: null,
    createdAt: new Date(),
  };
}

const projects = [
  createProject("1", "Alpha"),
  createProject("2", "Beta"),
  createProject("3", "Gamma"),
  createProject("4", "Delta"),
  createProject("5", "Epsilon"),
];

describe("ProjectSelector chip truncation", () => {
  afterEach(() => {
    cleanup();
  });

  it("should show all chips when 2 or fewer projects are selected", () => {
    // Given
    const selectedIds = ["1", "2"];

    // When
    render(
      <ProjectSelector
        multiple
        projects={projects}
        selectedProjectIds={selectedIds}
        onSelectionChange={vi.fn()}
      />,
    );

    // Then
    expect(screen.getByText("Alpha")).toBeTruthy();
    expect(screen.getByText("Beta")).toBeTruthy();
    expect(screen.queryByText(/^\+\d+$/)).toBeNull();
  });

  it("should show only 2 chips and a +N badge when 3 or more projects are selected", () => {
    // Given
    const selectedIds = ["1", "2", "3", "4", "5"];

    // When
    render(
      <ProjectSelector
        multiple
        projects={projects}
        selectedProjectIds={selectedIds}
        onSelectionChange={vi.fn()}
      />,
    );

    // Then
    expect(screen.getByText("Alpha")).toBeTruthy();
    expect(screen.getByText("Beta")).toBeTruthy();
    expect(screen.queryByText("Gamma")).toBeNull();
    expect(screen.queryByText("Delta")).toBeNull();
    expect(screen.queryByText("Epsilon")).toBeNull();
    expect(screen.getByText("+3")).toBeTruthy();
  });
});
