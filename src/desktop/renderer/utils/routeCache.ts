const ROUTE_CACHE_PREFIX = "kanvibe:route-cache";

export function buildRouteCacheKey(scope: string, id?: string): string {
  return id ? `${ROUTE_CACHE_PREFIX}:${scope}:${id}` : `${ROUTE_CACHE_PREFIX}:${scope}`;
}

export function readRouteCache<T>(key: string): T | null {
  try {
    const raw = sessionStorage.getItem(key);
    if (!raw) {
      return null;
    }

    return JSON.parse(raw) as T;
  } catch {
    return null;
  }
}

export function writeRouteCache<T>(key: string, value: T): void {
  try {
    sessionStorage.setItem(key, JSON.stringify(value));
  } catch {
    /* sessionStorage 접근 실패 시 캐시를 건너뛴다 */
  }
}

export function removeRouteCache(key: string): void {
  try {
    sessionStorage.removeItem(key);
  } catch {
    /* sessionStorage 접근 실패 시 캐시 삭제를 건너뛴다 */
  }
}
