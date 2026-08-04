import { describe, it, expect, vi, beforeEach } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import ProjectRegistryDialog from "../ProjectRegistryDialog";
import type { Project } from "@/entities/Project";

const projectActionMocks = vi.hoisted(() => ({
  deleteProject: vi.fn(),
  scanAndRegisterProjects: vi.fn(),
}));

const boardCommandMocks = vi.hoisted(() => ({
  registerShortcutBlocker: vi.fn(),
  unregisterShortcutBlocker: vi.fn(),
}));

vi.mock("next-intl", () => ({
  useTranslations: (namespace?: string) => (key: string, values?: Record<string, unknown>) => {
    if (namespace === "settings" && key === "registeredCount") {
      return `${values?.count} registered`;
    }
    if (namespace === "settings" && key === "worktreeTasksRegistered") {
      return `${values?.count} worktree tasks`;
    }
    return key;
  },
}));

vi.mock("@/components/FolderSearchInput", () => ({
  default: ({ name }: { name: string }) => (
    <input data-testid="folder-search-input" type="hidden" name={name} value="~/" />
  ),
}));

vi.mock("@/desktop/renderer/components/BoardCommandProvider", () => ({
  useBoardCommands: () => ({
    registerShortcutBlocker: boardCommandMocks.registerShortcutBlocker,
  }),
}));

vi.mock("@/desktop/renderer/actions/project", () => ({
  deleteProject: (...args: unknown[]) => projectActionMocks.deleteProject(...args),
  scanAndRegisterProjects: (...args: unknown[]) => projectActionMocks.scanAndRegisterProjects(...args),
}));

function createProject(overrides: Partial<Project> = {}): Project {
  return {
    id: "project-1",
    name: "kanvibe",
    repoPath: "/repo/kanvibe",
    defaultBranch: "main",
    sshHost: null,
    isWorktree: false,
    color: null,
    iconDataUrl: null,
    createdAt: new Date(),
    ...overrides,
  };
}

describe("ProjectRegistryDialog", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    boardCommandMocks.registerShortcutBlocker.mockReturnValue(boardCommandMocks.unregisterShortcutBlocker);
    projectActionMocks.deleteProject.mockResolvedValue(true);
    projectActionMocks.scanAndRegisterProjects.mockResolvedValue({
      registered: [],
      skipped: [],
      errors: [],
      worktreeTasks: [],
    });
  });

  it("닫혀 있으면 아무 것도 렌더링하지 않는다", () => {
    render(
      <ProjectRegistryDialog
        isOpen={false}
        onClose={vi.fn()}
        projects={[createProject()]}
        sshHosts={[]}
      />,
    );

    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("프로젝트 등록 dialog에 스캔 폼과 등록된 프로젝트 목록을 렌더링한다", () => {
    render(
      <ProjectRegistryDialog
        isOpen
        onClose={vi.fn()}
        projects={[createProject()]}
        sshHosts={["roky-home"]}
      />,
    );

    expect(screen.getByRole("dialog", { name: "scanTitle" })).toBeTruthy();
    expect(screen.getByTestId("folder-search-input")).toBeTruthy();
    expect(screen.getByRole("button", { name: "scanButton" })).toBeTruthy();
    expect(screen.getByText("projectList (1)")).toBeTruthy();
    expect(screen.getByText("kanvibe")).toBeTruthy();
  });

  it("열려 있는 동안 보드 단축키를 차단한다", () => {
    const { unmount } = render(
      <ProjectRegistryDialog
        isOpen
        onClose={vi.fn()}
        projects={[createProject()]}
        sshHosts={[]}
      />,
    );

    const dialog = screen.getByRole("dialog", { name: "scanTitle" });
    expect(dialog.closest('[data-shortcut-capture="true"]')).toBeTruthy();
    expect(boardCommandMocks.registerShortcutBlocker).toHaveBeenCalledTimes(1);

    unmount();

    expect(boardCommandMocks.unregisterShortcutBlocker).toHaveBeenCalledTimes(1);
  });

  it("스캔 폼을 제출하면 선택한 SSH host와 경로로 프로젝트를 등록한다", async () => {
    render(
      <ProjectRegistryDialog
        isOpen
        onClose={vi.fn()}
        projects={[createProject()]}
        sshHosts={["roky-home"]}
      />,
    );

    fireEvent.change(screen.getByRole("combobox"), { target: { value: "roky-home" } });
    fireEvent.submit(screen.getByTestId("project-registry-form"));

    await waitFor(() => {
      expect(projectActionMocks.scanAndRegisterProjects).toHaveBeenCalledWith("~/", "roky-home");
    });
  });

  it("삭제 확인 후 등록 프로젝트를 삭제한다", async () => {
    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(true);

    render(
      <ProjectRegistryDialog
        isOpen
        onClose={vi.fn()}
        projects={[createProject()]}
        sshHosts={[]}
      />,
    );

    try {
      fireEvent.click(screen.getByRole("button", { name: "deleteProject" }));

      expect(confirmSpy).toHaveBeenCalled();
      await waitFor(() => {
        expect(projectActionMocks.deleteProject).toHaveBeenCalledWith("project-1");
      });
    } finally {
      confirmSpy.mockRestore();
    }
  });

  it("프로젝트 이름을 검색하면 일치하는 프로젝트만 목록에 남긴다", () => {
    render(
      <ProjectRegistryDialog
        isOpen
        onClose={vi.fn()}
        projects={[
          createProject(),
          createProject({ id: "project-2", name: "timelabs", repoPath: "/repo/timelabs" }),
        ]}
        sshHosts={[]}
      />,
    );

    expect(screen.getByText("projectList (2)")).toBeTruthy();

    fireEvent.change(screen.getByTestId("project-registry-search"), {
      target: { value: "timel" },
    });

    expect(screen.getByText("timelabs")).toBeTruthy();
    expect(screen.queryByText("kanvibe")).toBeNull();
    expect(screen.getByText("projectList (1)")).toBeTruthy();
  });

  it("검색 결과가 없으면 일치 프로젝트 없음 안내를 보여준다", () => {
    render(
      <ProjectRegistryDialog
        isOpen
        onClose={vi.fn()}
        projects={[createProject()]}
        sshHosts={[]}
      />,
    );

    fireEvent.change(screen.getByTestId("project-registry-search"), {
      target: { value: "존재하지-않는-프로젝트" },
    });

    expect(screen.getByText("noMatchingProjects")).toBeTruthy();
    expect(screen.queryByText("kanvibe")).toBeNull();
    expect(screen.queryByText("noProjects")).toBeNull();
  });

  it("등록된 프로젝트가 없으면 검색 입력을 렌더링하지 않는다", () => {
    render(
      <ProjectRegistryDialog
        isOpen
        onClose={vi.fn()}
        projects={[]}
        sshHosts={[]}
      />,
    );

    expect(screen.queryByTestId("project-registry-search")).toBeNull();
    expect(screen.getByText("noProjects")).toBeTruthy();
  });

  it("Escape를 누르면 dialog를 닫는다", () => {
    const onClose = vi.fn();

    render(
      <ProjectRegistryDialog
        isOpen
        onClose={onClose}
        projects={[createProject()]}
        sshHosts={[]}
      />,
    );

    fireEvent.keyDown(window, { key: "Escape" });

    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
