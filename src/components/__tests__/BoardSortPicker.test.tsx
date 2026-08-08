import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import BoardSortPicker from "../BoardSortPicker";
import type { BoardSortPreference } from "@/desktop/shared/boardSort";
import enMessages from "../../../messages/en.json";

vi.mock("next-intl", () => ({
  useTranslations: (namespace: string) => (key: string, values?: Record<string, string>) => {
    const path = `${namespace}.${key}`.split(".");
    let node: unknown = enMessages;
    for (const segment of path) {
      node = (node as Record<string, unknown> | undefined)?.[segment];
    }

    return typeof node === "string"
      ? node.replace(/\{(\w+)\}/g, (_, name: string) => values?.[name] ?? `{${name}}`)
      : path.join(".");
  },
}));

const EMPTY_PREFERENCE: BoardSortPreference = { keys: [] };

async function openPicker(preference: BoardSortPreference) {
  const onChange = vi.fn();
  const user = userEvent.setup();
  render(<BoardSortPicker preference={preference} onChange={onChange} />);
  await user.click(screen.getByRole("button", { name: /Sort/ }));

  return { onChange, user };
}

describe("BoardSortPicker", () => {
  it("기준을 고르면 그 기준이 오름차순으로 쌓인다", async () => {
    // Given
    const { onChange, user } = await openPicker(EMPTY_PREFERENCE);

    // When
    await user.click(screen.getByRole("option", { name: /Priority/ }));

    // Then
    expect(onChange).toHaveBeenCalledWith({ keys: [{ field: "priority", direction: "asc" }] });
  });

  it("이미 고른 기준을 다시 누르면 빠진다", async () => {
    // Given
    const { onChange, user } = await openPicker({ keys: [{ field: "priority", direction: "asc" }] });

    // When
    await user.click(screen.getByRole("option", { name: /Priority/ }));

    // Then
    expect(onChange).toHaveBeenCalledWith({ keys: [] });
  });

  it("고른 기준의 방향을 뒤집는다", async () => {
    // Given
    const { onChange, user } = await openPicker({ keys: [{ field: "createdAt", direction: "asc" }] });

    // When
    await user.click(screen.getByRole("button", { name: /press to flip the direction/ }));

    // Then
    expect(onChange).toHaveBeenCalledWith({ keys: [{ field: "createdAt", direction: "desc" }] });
  });

  it("정렬 해제는 고른 기준을 모두 비운다", async () => {
    // Given
    const { onChange, user } = await openPicker({ keys: [{ field: "title", direction: "desc" }] });

    // When
    await user.click(screen.getByRole("button", { name: enMessages.board.sort.clear }));

    // Then
    expect(onChange).toHaveBeenCalledWith({ keys: [] });
  });

  it("고른 기준이 없으면 정렬 해제를 누를 수 없다", async () => {
    // Given / When
    await openPicker(EMPTY_PREFERENCE);

    // Then
    const clearButton = screen.getByRole("button", { name: enMessages.board.sort.clear });
    expect((clearButton as HTMLButtonElement).disabled).toBe(true);
  });

  it("카드 자리를 고르는 모드는 더 이상 제공하지 않는다", async () => {
    // Given / When
    await openPicker(EMPTY_PREFERENCE);

    // Then
    /** 보드 순서는 고른 기준으로만 정해지므로 자리와 기준 중 무엇을 먼저 볼지 물을 것이 없다 */
    expect(screen.queryAllByRole("radio")).toHaveLength(0);
  });

  it("고른 기준 개수를 트리거에 표시한다", async () => {
    // Given / When
    await openPicker({
      keys: [
        { field: "priority", direction: "asc" },
        { field: "title", direction: "asc" },
      ],
    });

    // Then
    expect(screen.getByTestId("board-sort-active-count").textContent).toBe("2");
  });
});
