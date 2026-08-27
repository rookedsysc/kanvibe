import { AI_CRAWLER_USER_AGENTS, resolveDocsSiteUrl } from '../lib/docsSite.mjs'

/**
 * Google이 sitemap ping 엔드포인트를 폐기한 뒤로 robots.txt의 Sitemap 줄이
 * 인증 없이 sitemap을 알릴 수 있는 유일한 경로다.
 */
export default function robots() {
  const siteUrl = resolveDocsSiteUrl()

  return {
    rules: [
      { userAgent: '*', allow: '/' },
      { userAgent: AI_CRAWLER_USER_AGENTS, allow: '/' }
    ],
    sitemap: `${siteUrl}/sitemap.xml`
  }
}
