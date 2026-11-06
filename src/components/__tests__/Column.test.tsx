import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import Column from "../Column";
import { TaskStatus, type KanbanTask } from "@/entities/KanbanTask";

vi.mock("next-intl", () => ({ useTranslations: () => (key: string) => key }));
vi.mock("@hello-pangea/dnd", () => ({
  Droppable: ({ children }: { children: (provided: unknown, snapshot: unknown) => React.ReactNode }) => children({ innerRef: vi.fn(), droppableProps: {}, placeholder: null }, {}),
}));
vi.mock("@/desktop/renderer/hooks/useFlipReflow", () => ({ useFlipReflow: () => null }));
vi.mock("../ProjectTaskGroup", () => ({ default: ({ children }: { children: React.ReactNode }) => <section data-testid="project-group">{children}</section> }));
vi.mock("../TaskCard", () => ({
  default: ({ task, projectColor, projectIconDataUrl, index }: { task: KanbanTask; projectColor: string; projectIconDataUrl: string; index: number }) => <span data-testid={task.id} data-color={projectColor} data-icon={projectIconDataUrl} data-index={index}>{task.title}</span>,
}));

describe("Column project display identity", () => {
  it("separates same-name host projects while grouping their related worktrees", () => {
    const tasks = ["local", "local-wt", "remote", "remote-wt"].map((id) => ({ id, title: id, projectId: id, status: TaskStatus.TODO }) as KanbanTask);
    render(<Column status={TaskStatus.TODO} tasks={tasks} label="Todo" colorClass="" onContextMenu={vi.fn()}
      projectNameMap={{ local: "repo", "local-wt": "repo", remote: "repo", "remote-wt": "repo" }}
      projectGroupMap={{ local: "local", "local-wt": "local", remote: "remote", "remote-wt": "remote" }}
      projectColorMap={{ local: "#ff0000", "local-wt": "#ff0000", remote: "#0000ff", "remote-wt": "#0000ff" }}
      projectIconMap={{ local: "local-icon", "local-wt": "local-icon", remote: "remote-icon", "remote-wt": "remote-icon" }} />);
    const groups = screen.getAllByTestId("project-group");
    expect(groups).toHaveLength(2);
    expect(groups[0].contains(screen.getByTestId("local-wt"))).toBe(true);
    expect(groups[1].contains(screen.getByTestId("remote-wt"))).toBe(true);
    for (const [index, task] of tasks.entries()) {
      const card = screen.getByTestId(task.id);
      expect(card.getAttribute("data-color")).toBe(task.id.startsWith("local") ? "#ff0000" : "#0000ff");
      expect(card.getAttribute("data-icon")).toBe(task.id.startsWith("local") ? "local-icon" : "remote-icon");
      expect(card.getAttribute("data-index")).toBe(String(index));
    }
  });
});
