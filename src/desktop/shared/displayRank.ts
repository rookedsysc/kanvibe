/**
 * 보드 카드의 수동 배치 순서를 나타내는 16진 fractional rank.
 *
 * 정수 순번과 달리 두 카드 사이에 언제든 새 값을 만들 수 있어서, 카드 하나를 옮길 때
 * 컬럼 전체를 다시 번호 매기지 않고 옮긴 행 하나만 갱신하면 된다.
 * 값은 소수점 이하 자릿수로 해석한다. 예를 들어 "8"은 0.8(16진), "8c"는 0.8c다.
 */

const RANK_DIGITS = "0123456789abcdef";
const SMALLEST_DIGIT = RANK_DIGITS[0];
const RANK_BASE = RANK_DIGITS.length;

/** 마지막 자리가 0이면 그보다 작은 값을 만들 수 없어 rank 생성이 막히므로, 모든 rank는 0으로 끝나지 않아야 한다 */
function hasTrailingZero(rank: string): boolean {
  return rank.endsWith(SMALLEST_DIGIT);
}

function assertValidRank(rank: string, label: string): void {
  if (!rank) {
    throw new Error(`${label} rank는 빈 문자열일 수 없습니다.`);
  }
  if (hasTrailingZero(rank)) {
    throw new Error(`${label} rank "${rank}"는 0으로 끝나 사이 값을 만들 수 없습니다.`);
  }
  for (const character of rank) {
    if (!RANK_DIGITS.includes(character)) {
      throw new Error(`${label} rank "${rank}"에 16진 문자가 아닌 값이 있습니다.`);
    }
  }
}

/**
 * 두 rank 사이의 값을 만든다. lower가 빈 문자열이면 맨 앞, upper가 null이면 맨 뒤를 의미한다.
 * 공통 접두사를 잘라내고 첫 다른 자리에서 중간값을 고르는 방식이라 자릿수가 필요 이상으로 늘지 않는다.
 */
function midpoint(lower: string, upper: string | null): string {
  if (upper !== null) {
    const commonPrefixLength = countCommonPrefix(lower, upper);
    if (commonPrefixLength > 0) {
      return upper.slice(0, commonPrefixLength)
        + midpoint(lower.slice(commonPrefixLength), upper.slice(commonPrefixLength));
    }
  }

  const lowerDigit = lower ? RANK_DIGITS.indexOf(lower[0]) : 0;
  const upperDigit = upper !== null ? RANK_DIGITS.indexOf(upper[0]) : RANK_BASE;

  const hasRoomBetweenDigits = upperDigit - lowerDigit > 1;
  if (hasRoomBetweenDigits) {
    return RANK_DIGITS[Math.round((lowerDigit + upperDigit) / 2)];
  }

  // 첫 자리가 붙어 있으면 한 자리를 더 내려가서 사이 값을 찾는다
  if (upper !== null && upper.length > 1) {
    return upper.slice(0, 1);
  }
  return RANK_DIGITS[lowerDigit] + midpoint(lower.slice(1), null);
}

function countCommonPrefix(lower: string, upper: string): number {
  let length = 0;
  while ((lower[length] ?? SMALLEST_DIGIT) === upper[length]) {
    length += 1;
  }
  return length;
}

/**
 * 앞 rank와 뒤 rank 사이에 놓일 새 rank를 만든다.
 * 앞이 null이면 맨 앞, 뒤가 null이면 맨 뒤에 놓는다.
 */
export function rankBetween(previousRank: string | null, nextRank: string | null): string {
  if (previousRank !== null) {
    assertValidRank(previousRank, "앞");
  }
  if (nextRank !== null) {
    assertValidRank(nextRank, "뒤");
  }
  if (previousRank !== null && nextRank !== null && previousRank >= nextRank) {
    throw new Error(`앞 rank "${previousRank}"가 뒤 rank "${nextRank}"보다 작아야 합니다.`);
  }

  return midpoint(previousRank ?? "", nextRank);
}

/**
 * 이미 순서가 정해진 목록 전체에 rank를 한 번에 부여한다.
 * 자릿수를 같게 맞춰 사전순 비교가 곧 목록 순서가 되도록 하고, 0으로 끝나는 값은 뒤에 한 자리를 붙여 피한다.
 */
export function buildSequentialRanks(count: number): string[] {
  if (count <= 0) {
    return [];
  }

  const width = Math.max(1, Math.ceil(Math.log(count + 1) / Math.log(RANK_BASE)));
  const ranks: string[] = [];

  for (let position = 1; position <= count; position += 1) {
    const padded = position.toString(RANK_BASE).padStart(width, SMALLEST_DIGIT);
    ranks.push(hasTrailingZero(padded) ? padded + "8" : padded);
  }

  return ranks;
}

/** rank 오름차순 비교자. 값이 없는 항목은 방향과 무관하게 항상 뒤로 보낸다 */
export function compareDisplayRank(left: string | null, right: string | null): number {
  if (left === right) return 0;
  if (left === null) return 1;
  if (right === null) return -1;
  return left < right ? -1 : 1;
}
