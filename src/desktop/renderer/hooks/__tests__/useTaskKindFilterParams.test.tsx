import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter, useSearchParams } from "react-router-dom";
import { describe, expect, it, beforeEach } from "vitest";
import { useTaskKindFilterParams } from "../useTaskKindFilterParams";

function TaskKindFilterHarness() {
  const [filter, setFilter] = useTaskKindFilterParams();
  const [searchParams] = useSearchParams();

  return (
    <div>
      <div data-testid="filter">{filter}</div>
      <div data-testid="query">{searchParams.get("taskKind") ?? ""}</div>
      <button type="button" onClick={() => setFilter("project")}>project</button>
      <button type="button" onClick={() => setFilter("task")}>task</button>
      <button type="button" onClick={() => setFilter("all")}>all</button>
    </div>
  );
}

function renderHarness(initialEntry = "/ko") {
  return render(
    <MemoryRouter initialEntries={[initialEntry]}>
      <TaskKindFilterHarness />
    </MemoryRouter>,
  );
}

describe("useTaskKindFilterParams", () => {
  beforeEach(() => {
    sessionStorage.clear();
  });

  it("taskKind query가 없으면 All을 기본값으로 사용한다", () => {
    renderHarness();

    expect(screen.getByTestId("filter").textContent).toBe("all");
    expect(screen.getByTestId("query").textContent).toBe("");
  });

  it("Project/Task 선택을 URL query와 sessionStorage에 반영하고 All 선택 시 query를 제거한다", async () => {
    renderHarness();

    fireEvent.click(screen.getByRole("button", { name: "project" }));

    await waitFor(() => {
      expect(screen.getByTestId("filter").textContent).toBe("project");
      expect(screen.getByTestId("query").textContent).toBe("project");
    });
    expect(sessionStorage.getItem("kanvibe_task_kind_filter")).toBe("project");

    fireEvent.click(screen.getByRole("button", { name: "task" }));

    await waitFor(() => {
      expect(screen.getByTestId("filter").textContent).toBe("task");
      expect(screen.getByTestId("query").textContent).toBe("task");
    });
    expect(sessionStorage.getItem("kanvibe_task_kind_filter")).toBe("task");

    fireEvent.click(screen.getByRole("button", { name: "all" }));

    await waitFor(() => {
      expect(screen.getByTestId("filter").textContent).toBe("all");
      expect(screen.getByTestId("query").textContent).toBe("");
    });
    expect(sessionStorage.getItem("kanvibe_task_kind_filter")).toBeNull();
  });

  it("query가 없으면 sessionStorage에 저장된 필터를 복원한다", async () => {
    sessionStorage.setItem("kanvibe_task_kind_filter", "task");

    renderHarness();

    await waitFor(() => {
      expect(screen.getByTestId("filter").textContent).toBe("task");
      expect(screen.getByTestId("query").textContent).toBe("task");
    });
  });

  it("알 수 없는 query 값은 제거하고 기본값으로 되돌린다", async () => {
    renderHarness("/ko?taskKind=unknown");

    await waitFor(() => {
      expect(screen.getByTestId("filter").textContent).toBe("all");
      expect(screen.getByTestId("query").textContent).toBe("");
    });
  });
});
