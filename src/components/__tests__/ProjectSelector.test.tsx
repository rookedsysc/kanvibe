import { createRef } from "react";
import { describe, it, expect, vi, afterEach } from "vitest";
import { act, render, screen, cleanup, fireEvent } from "@testing-library/react";
import type { Project } from "@/entities/Project";

vi.mock("next-intl", () => ({
  useTranslations: () => (key: string) => key,
}));

import ProjectSelector, { type ProjectSelectorHandle } from "../ProjectSelector";

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

function openSelectorAndSearch(query: string, projectsForTest = projects) {
  render(
    <ProjectSelector
      multiple
      projects={projectsForTest}
      selectedProjectIds={[]}
      onSelectionChange={vi.fn()}
      placeholder="All projects"
      searchPlaceholder="Search project..."
    />,
  );

  fireEvent.click(screen.getByText("All projects"));
  fireEvent.change(screen.getByPlaceholderText("Search project..."), {
    target: { value: query },
  });
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

  it("should match remote projects by ssh host", () => {
    const selectorProjects = [
      createProject("local", "Local API"),
      {
        ...createProject("remote", "Remote API"),
        repoPath: "/srv/team/remote-api",
        sshHost: "remote-host",
      },
    ];

    openSelectorAndSearch("remote-host", selectorProjects);

    expect(screen.getByText("Remote API")).toBeTruthy();
    expect(screen.queryByText("Local API")).toBeNull();
  });

  it("should match projects by repo path", () => {
    const selectorProjects = [
      createProject("local", "Local API"),
      {
        ...createProject("remote", "Remote API"),
        repoPath: "/srv/team/remote-api",
        sshHost: "remote-host",
      },
    ];

    openSelectorAndSearch("/srv/team", selectorProjects);

    expect(screen.getByText("Remote API")).toBeTruthy();
    expect(screen.queryByText("Local API")).toBeNull();
  });

  it("should keep selected projects at the top and toggle the highlighted project after imperative open", () => {
    const ref = createRef<ProjectSelectorHandle>();
    const onSelectionChange = vi.fn();

    render(
      <ProjectSelector
        ref={ref}
        multiple
        projects={projects}
        selectedProjectIds={["2"]}
        onSelectionChange={onSelectionChange}
        placeholder="All projects"
        searchPlaceholder="Search project..."
      />,
    );

    act(() => {
      ref.current?.open();
    });

    const searchInput = screen.getByPlaceholderText("Search project...");
    const items = screen.getAllByRole("listitem");
    expect(items[0].textContent).toContain("Beta");

    fireEvent.keyDown(searchInput, { key: "Enter" });

    expect(onSelectionChange).toHaveBeenCalledWith([]);
  });
});
