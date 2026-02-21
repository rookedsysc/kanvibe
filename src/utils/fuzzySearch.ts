export interface FuzzyMatch {
  path: string;
  score: number;
  /** 매칭된 문자의 인덱스 배열 */
  matchedIndices: number[];
}

/** fzf 스타일 subsequence fuzzy matching을 수행한다 */
export function fuzzyMatch(query: string, target: string): FuzzyMatch | null {
  const lowerQuery = query.toLowerCase();
  const lowerTarget = target.toLowerCase();
  const matchedIndices: number[] = [];

  let queryIdx = 0;
  for (let i = 0; i < lowerTarget.length && queryIdx < lowerQuery.length; i++) {
    if (lowerTarget[i] === lowerQuery[queryIdx]) {
      matchedIndices.push(i);
      queryIdx++;
    }
  }

  if (queryIdx !== lowerQuery.length) return null;

  /** 점수 계산: 연속 매칭 보너스 + 경로 끝부분(폴더명) 매칭 가중치 */
  let score = 0;
  const lastSlash = target.lastIndexOf("/");

  for (let i = 0; i < matchedIndices.length; i++) {
    const idx = matchedIndices[i];
    if (idx > lastSlash) score += 2;
    else score += 1;

    if (i > 0 && matchedIndices[i] === matchedIndices[i - 1] + 1) {
      score += 3;
    }
  }

  return { path: target, score, matchedIndices };
}
