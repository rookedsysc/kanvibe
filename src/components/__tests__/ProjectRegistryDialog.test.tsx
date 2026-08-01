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

function createProject(): Project {
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
