import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter, useLocation } from "react-router-dom";
import { beforeEach, describe, expect, it } from "vitest";
import { useRouter } from "@/desktop/renderer/navigation";

function RouterProbe() {
  const router = useRouter();
  const location = useLocation();

  return (
    <>
      <div data-testid="pathname">{location.pathname}</div>
      <button type="button" onClick={() => router.back()}>back</button>
    </>
  );
}

function renderRouterProbe(initialEntries: string[], initialIndex: number, historyState: unknown) {
  window.history.replaceState(historyState, "", "/");

  render(
    <MemoryRouter initialEntries={initialEntries} initialIndex={initialIndex}>
      <RouterProbe />
    </MemoryRouter>,
  );
}

describe("useRouter", () => {
  beforeEach(() => {
    window.history.replaceState(null, "", "/");
  });

  it("뒤로 갈 히스토리가 있으면 이전 페이지로 이동한다", async () => {
    renderRouterProbe(["/ko", "/ko/task/task-1"], 1, { idx: 1 });

    fireEvent.click(screen.getByRole("button", { name: "back" }));

    await waitFor(() => {
      expect(screen.getByTestId("pathname").textContent).toBe("/ko");
    });
  });

  it("뒤로 갈 히스토리가 없으면 현재 locale의 칸반 홈으로 이동한다", async () => {
    renderRouterProbe(["/en/task/previous", "/ko/task/task-1"], 1, { idx: 0 });

    fireEvent.click(screen.getByRole("button", { name: "back" }));

    await waitFor(() => {
      expect(screen.getByTestId("pathname").textContent).toBe("/ko");
    });
  });

  it("히스토리 index를 알 수 없으면 현재 locale의 칸반 홈으로 이동한다", async () => {
    renderRouterProbe(["/en/task/previous", "/zh/task/task-1"], 1, null);

    fireEvent.click(screen.getByRole("button", { name: "back" }));

    await waitFor(() => {
      expect(screen.getByTestId("pathname").textContent).toBe("/zh");
    });
  });
});
