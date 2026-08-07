/**
 * 보드 정렬 순서 필터의 공유 어휘.
 * 정렬 기준을 고른 순서대로 쌓고 각 기준마다 방향을 뒤집을 수 있으며,
 * 수동 배치 순서와 어떻게 겹칠지는 mode가 정한다.
 * main의 설정 저장과 renderer의 정렬 적용이 같은 정의를 쓰도록 여기에 한 번만 둔다.
 */

export const BOARD_SORT_FIELDS = ["priority", "createdAt", "updatedAt", "title", "project"] as const;
export type BoardSortField = typeof BOARD_SORT_FIELDS[number];

export const BOARD_SORT_DIRECTIONS = ["asc", "desc"] as const;
export type BoardSortDirection = typeof BOARD_SORT_DIRECTIONS[number];

/** 수동 배치 순서(display rank)와 정렬 기준 중 무엇을 먼저 적용할지 */
export const BOARD_SORT_MODES = ["manual-first", "sort-first", "manual-off"] as const;
export type BoardSortMode = typeof BOARD_SORT_MODES[number];

export interface BoardSortKey {
  field: BoardSortField;
  direction: BoardSortDirection;
}

export interface BoardSortPreference {
  keys: BoardSortKey[];
  mode: BoardSortMode;
}

export const DEFAULT_BOARD_SORT_PREFERENCE: BoardSortPreference = {
  keys: [],
  mode: "sort-first",
};

function isBoardSortField(value: unknown): value is BoardSortField {
  return typeof value === "string" && BOARD_SORT_FIELDS.includes(value as BoardSortField);
}

function isBoardSortDirection(value: unknown): value is BoardSortDirection {
  return typeof value === "string" && BOARD_SORT_DIRECTIONS.includes(value as BoardSortDirection);
}

function isBoardSortMode(value: unknown): value is BoardSortMode {
  return typeof value === "string" && BOARD_SORT_MODES.includes(value as BoardSortMode);
}

/** 저장된 값에서 알아볼 수 있는 기준만 원래 순서대로 추린다. 같은 기준이 두 번 나오면 앞의 것만 남긴다 */
function parseSortKeys(rawKeys: unknown): BoardSortKey[] {
  if (!Array.isArray(rawKeys)) {
    return [];
  }

  const keys: BoardSortKey[] = [];
  const seenFields = new Set<BoardSortField>();

  for (const rawKey of rawKeys) {
    if (typeof rawKey !== "object" || rawKey === null) continue;

    const { field, direction } = rawKey as { field?: unknown; direction?: unknown };
    if (!isBoardSortField(field) || seenFields.has(field)) continue;

    seenFields.add(field);
    keys.push({ field, direction: isBoardSortDirection(direction) ? direction : "asc" });
  }

  return keys;
}

/** 저장된 문자열을 정렬 설정으로 읽는다. 읽을 수 없으면 기본값으로 되돌린다 */
export function parseBoardSortPreference(rawValue: string | null): BoardSortPreference {
  if (!rawValue) {
    return DEFAULT_BOARD_SORT_PREFERENCE;
  }

  try {
    const parsed: unknown = JSON.parse(rawValue);
    if (typeof parsed !== "object" || parsed === null) {
      return DEFAULT_BOARD_SORT_PREFERENCE;
    }

    const { keys, mode } = parsed as { keys?: unknown; mode?: unknown };
    return {
      keys: parseSortKeys(keys),
      mode: isBoardSortMode(mode) ? mode : DEFAULT_BOARD_SORT_PREFERENCE.mode,
    };
  } catch {
    return DEFAULT_BOARD_SORT_PREFERENCE;
  }
}

export function serializeBoardSortPreference(preference: BoardSortPreference): string {
  return JSON.stringify({ keys: preference.keys, mode: preference.mode });
}
