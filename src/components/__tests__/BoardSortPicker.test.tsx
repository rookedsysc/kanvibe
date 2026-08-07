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

const EMPTY_PREFERENCE: BoardSortPreference = { keys: [], mode: "sort-first" };

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
    expect(onChange).toHaveBeenCalledWith({
      keys: [{ field: "priority", direction: "asc" }],
      mode: "sort-first",
    });
  });

  it("이미 고른 기준을 다시 누르면 빠진다", async () => {
    // Given
    const { onChange, user } = await openPicker({
      keys: [{ field: "priority", direction: "asc" }],
      mode: "sort-first",
    });

    // When
    await user.click(screen.getByRole("option", { name: /Priority/ }));

    // Then
    expect(onChange).toHaveBeenCalledWith({ keys: [], mode: "sort-first" });
  });

  it("고른 기준의 방향을 뒤집는다", async () => {
    // Given
    const { onChange, user } = await openPicker({
      keys: [{ field: "createdAt", direction: "asc" }],
      mode: "sort-first",
    });

    // When
    await user.click(screen.getByRole("button", { name: /press to flip the direction/ }));

    // Then
    expect(onChange).toHaveBeenCalledWith({
      keys: [{ field: "createdAt", direction: "desc" }],
      mode: "sort-first",
    });
  });

  it("정렬 해제는 고른 기준만 비우고 모드는 남긴다", async () => {
    // Given
    const { onChange, user } = await openPicker({
      keys: [{ field: "title", direction: "desc" }],
      mode: "manual-first",
    });

    // When
    await user.click(screen.getByRole("button", { name: enMessages.board.sort.clear }));

    // Then
    expect(onChange).toHaveBeenCalledWith({ keys: [], mode: "manual-first" });
  });

  it("고른 기준이 없으면 정렬 해제를 누를 수 없다", async () => {
    // Given / When
    await openPicker(EMPTY_PREFERENCE);

    // Then
    const clearButton = screen.getByRole("button", { name: enMessages.board.sort.clear });
    expect((clearButton as HTMLButtonElement).disabled).toBe(true);
  });

  it("모드를 바꾸면 그 모드가 어떻게 정렬하는지 설명이 함께 바뀐다", async () => {
    // Given
    const { onChange, user } = await openPicker(EMPTY_PREFERENCE);
    expect(screen.getByTestId("board-sort-mode-description").textContent)
      .toBe(enMessages.board.sort.modeDescriptions["sort-first"]);

    // When
    await user.click(screen.getByRole("radio", { name: enMessages.board.sort.modes["manual-off"] }));

    // Then
    expect(onChange).toHaveBeenCalledWith({ keys: [], mode: "manual-off" });
  });

  it("고른 기준 개수를 트리거에 표시한다", async () => {
    // Given / When
    await openPicker({
      keys: [
        { field: "priority", direction: "asc" },
        { field: "title", direction: "asc" },
      ],
      mode: "sort-first",
    });

    // Then
    expect(screen.getByTestId("board-sort-active-count").textContent).toBe("2");
  });
});
