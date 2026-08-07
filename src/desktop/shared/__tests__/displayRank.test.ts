import { describe, expect, it } from "vitest";
import { buildSequentialRanks, compareDisplayRank, rankBetween } from "@/desktop/shared/displayRank";

describe("rankBetween", () => {
  it("앞뒤가 모두 비어 있으면 다음 값을 양쪽으로 만들 수 있는 가운데 값을 준다", () => {
    // Given / When
    const rank = rankBetween(null, null);

    // Then
    expect(rankBetween(null, rank) < rank).toBe(true);
    expect(rankBetween(rank, null) > rank).toBe(true);
  });

  it("앞만 주어지면 그보다 뒤에, 뒤만 주어지면 그보다 앞에 놓는다", () => {
    // Given
    const anchor = "8";

    // When
    const after = rankBetween(anchor, null);
    const before = rankBetween(null, anchor);

    // Then
    expect(after > anchor).toBe(true);
    expect(before < anchor).toBe(true);
  });

  it("붙어 있는 두 값 사이에도 자릿수를 늘려 값을 만든다", () => {
    // Given
    const previousRank = "8";
    const nextRank = "9";

    // When
    const rank = rankBetween(previousRank, nextRank);

    // Then
    expect(rank > previousRank).toBe(true);
    expect(rank < nextRank).toBe(true);
  });

  it("같은 자리에 50번을 연달아 끼워 넣어도 순서가 어긋나거나 값이 겹치지 않는다", () => {
    // Given
    const lowerBound = "2";
    const upperBound = "4";
    const inserted: string[] = [];
    let previousRank = lowerBound;

    // When
    for (let attempt = 0; attempt < 50; attempt += 1) {
      const rank = rankBetween(previousRank, upperBound);
      inserted.push(rank);
      previousRank = rank;
    }

    // Then
    expect(new Set(inserted).size).toBe(inserted.length);
    expect([...inserted].sort()).toEqual(inserted);
    expect(inserted[0] > lowerBound).toBe(true);
    expect(inserted.at(-1)! < upperBound).toBe(true);
  });

  it("앞이 뒤보다 크면 사이 값을 만들 수 없으므로 거부한다", () => {
    // Given / When / Then
    expect(() => rankBetween("9", "8")).toThrow();
  });

  it("0으로 끝나는 값은 그보다 작은 값을 만들 수 없으므로 거부한다", () => {
    // Given / When / Then
    expect(() => rankBetween("10", null)).toThrow();
  });
});

describe("buildSequentialRanks", () => {
  it("주어진 개수만큼 사전순으로 커지는 값을 만든다", () => {
    // Given / When
    const ranks = buildSequentialRanks(40);

    // Then
    expect(ranks).toHaveLength(40);
    expect([...ranks].sort()).toEqual(ranks);
  });

  it("만든 값 사이에 그대로 새 값을 끼워 넣을 수 있다", () => {
    // Given
    const ranks = buildSequentialRanks(40);

    // When
    const insertedRanks = ranks.slice(0, -1).map((rank, index) => rankBetween(rank, ranks[index + 1]));

    // Then
    insertedRanks.forEach((rank, index) => {
      expect(rank > ranks[index]).toBe(true);
      expect(rank < ranks[index + 1]).toBe(true);
    });
  });

  it("개수가 0이면 빈 목록을 준다", () => {
    // Given / When / Then
    expect(buildSequentialRanks(0)).toEqual([]);
  });
});

describe("compareDisplayRank", () => {
  it("값이 없는 항목은 오름차순에서 뒤로 간다", () => {
    // Given
    const ranks: (string | null)[] = ["8", null, "2", null, "4"];

    // When
    const sorted = [...ranks].sort(compareDisplayRank);

    // Then
    expect(sorted).toEqual(["2", "4", "8", null, null]);
  });
});
