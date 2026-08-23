import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import { CountUpNumber } from "@/components/CountUpNumber";

function mockReducedMotion(prefersReduced: boolean) {
  window.matchMedia = vi.fn().mockImplementation((query: string) => ({
    matches: prefersReduced && query === "(prefers-reduced-motion: reduce)",
    media: query,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
  })) as unknown as typeof window.matchMedia;
}

describe("CountUpNumber", () => {
  beforeEach(() => {
    mockReducedMotion(false);
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("첫 렌더에서는 굴리지 않고 받은 값을 그대로 보여준다", () => {
    render(<CountUpNumber value={348} testId="count" />);

    expect(screen.getByTestId("count").textContent).toBe("348");
    expect(screen.getByTestId("count").getAttribute("data-counting")).toBeNull();
  });

  it("값이 바뀌면 굴러가는 동안 표시하고 결국 새 값에 도달한다", async () => {
    const { rerender } = render(<CountUpNumber value={340} testId="count" />);

    rerender(<CountUpNumber value={348} testId="count" />);

    expect(screen.getByTestId("count").getAttribute("data-counting")).toBe("true");
    await waitFor(() => {
      expect(screen.getByTestId("count").textContent).toBe("348");
    });
    await waitFor(() => {
      expect(screen.getByTestId("count").getAttribute("data-counting")).toBeNull();
    });
  });

  it("움직임을 줄이도록 설정했으면 굴리지 않고 새 값으로 바로 바꾼다", () => {
    mockReducedMotion(true);
    const { rerender } = render(<CountUpNumber value={340} testId="count" />);

    rerender(<CountUpNumber value={348} testId="count" />);

    expect(screen.getByTestId("count").textContent).toBe("348");
    expect(screen.getByTestId("count").getAttribute("data-counting")).toBeNull();
  });
});
