import { headers } from 'next/headers'

import { resolvePinnedSiteUrl, resolveSiteUrlFromRequestHeaders } from '../lib/docsSite.mjs'

/**
 * 고정 주소가 정해져 있으면 그것을, 아니면 요청을 받은 호스트를 출처로 쓴다.
 * `headers()`를 읽는 순간 이 라우트들은 요청 시점 실행으로 바뀌는데,
 * OpenNext가 어차피 서버 함수로 태우므로 실제 동작과 어긋나지 않는다.
 */
export async function resolveDocsSiteUrl() {
  return resolvePinnedSiteUrl() ?? resolveSiteUrlFromRequestHeaders(await headers())
}
