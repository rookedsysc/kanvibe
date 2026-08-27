/**
 * 문서 사이트의 로케일, 페이지 목록, 정규 주소를 한곳에서 정의한다.
 * sitemap, robots, 페이지 metadata, next.config가 모두 이 값을 공유해야
 * hreflang 상호 참조와 canonical이 어긋나지 않는다.
 */

export const DEFAULT_LOCALE = 'ko'

export const DOCS_LOCALES = ['ko', 'en', 'zh']

/** 로케일 접두어를 뺀 문서 페이지 경로다. content/<locale> 아래 실제 MDX 파일과 일치해야 한다. */
export const DOCS_PAGE_PATHS = [
  '/',
  '/installation',
  '/quick-start',
  '/features',
  '/settings',
  '/shortcuts'
]

/**
 * 생성형 검색에 문서를 노출시키려면 이 크롤러들이 막히지 않아야 한다.
 * 검색용과 학습용을 모두 허용하는 것이 KanVibe 문서의 정책이며,
 * 정책을 바꾸려면 이 목록을 나눠서 규칙을 따로 주면 된다.
 */
export const AI_CRAWLER_USER_AGENTS = [
  'OAI-SearchBot',
  'ChatGPT-User',
  'GPTBot',
  'Claude-SearchBot',
  'Claude-User',
  'ClaudeBot',
  'PerplexityBot',
  'Google-Extended'
]

function normalizeSiteUrl(rawSiteUrl) {
  const parsedSiteUrl = new URL(rawSiteUrl)
  const basePath = parsedSiteUrl.pathname.replace(/\/+$/, '')
  return `${parsedSiteUrl.origin}${basePath}`
}

/**
 * 문서 사이트를 여러 호스트가 서빙할 때 어느 쪽으로 신호를 모을지 정하는 값이다.
 * workers.dev 주소와 커스텀 도메인이 같은 문서를 함께 내보내는 상황에서만 의미가 있고,
 * 값이 정해지기 전까지는 요청을 받은 호스트를 그대로 쓰는 편이 틀린 주소를 박는 것보다 낫다.
 *
 * 기본값을 `process.env` 멤버 접근 그대로 두는 것이 중요하다.
 * next.config의 `env`가 이 참조를 빌드 시점 값으로 치환하기 때문에,
 * Cloudflare Worker 런타임에는 환경변수 없이도 주소가 남는다.
 */
export function resolvePinnedSiteUrl(pinnedSiteUrl = process.env.KANVIBE_DOCS_SITE_URL) {
  const trimmedSiteUrl = pinnedSiteUrl?.trim()
  if (!trimmedSiteUrl) {
    return undefined
  }
  return normalizeSiteUrl(trimmedSiteUrl)
}

/**
 * 요청을 받은 호스트에서 출처를 읽는다.
 * sitemap의 loc은 sitemap 자신과 같은 호스트여야 하므로, 고정 주소가 없을 때는
 * 실제로 응답한 호스트가 언제나 정답이다.
 */
export function resolveSiteUrlFromRequestHeaders(requestHeaders) {
  const host = requestHeaders.get('x-forwarded-host') ?? requestHeaders.get('host')
  if (!host) {
    return undefined
  }

  const forwardedProtocol = requestHeaders.get('x-forwarded-proto')
  const protocol = forwardedProtocol ?? (/^(localhost|127\.|\[::1\])/.test(host) ? 'http' : 'https')
  return normalizeSiteUrl(`${protocol}://${host}`)
}

export function buildDocsPageUrl(siteUrl, locale, pagePath) {
  const pathSuffix = pagePath === '/' ? '' : pagePath
  return `${siteUrl}/${locale}${pathSuffix}`
}

/**
 * 한 페이지의 모든 언어 변형을 hreflang 맵으로 만든다.
 * Google은 각 변형이 자기 자신을 포함해 모든 변형을 가리킬 때만 hreflang을 인정하므로
 * 호출한 로케일 자신도 반드시 포함된다.
 */
export function buildLanguageAlternates(siteUrl, pagePath) {
  const languageAlternates = {}
  for (const locale of DOCS_LOCALES) {
    languageAlternates[locale] = buildDocsPageUrl(siteUrl, locale, pagePath)
  }
  languageAlternates['x-default'] = buildDocsPageUrl(siteUrl, DEFAULT_LOCALE, pagePath)
  return languageAlternates
}

/** Nextra가 넘기는 mdxPath 세그먼트 배열을 문서 페이지 경로로 바꾼다. */
export function toDocsPagePath(mdxPathSegments) {
  if (!mdxPathSegments || mdxPathSegments.length === 0) {
    return '/'
  }
  return `/${mdxPathSegments.join('/')}`
}

/**
 * sitemap 항목을 만든다.
 *
 * priority와 changefreq는 Google이 무시한다고 명시했고, lastmod는 정확할 때만 쓰이는데
 * 이 라우트는 Cloudflare Worker에서 요청마다 다시 실행되어 git 이력을 읽을 수 없다.
 * 부정확한 lastmod는 없느니만 못하다는 것이 Google의 안내라 세 값을 모두 넣지 않는다.
 */
export function buildDocsSitemapEntries(siteUrl) {
  return DOCS_LOCALES.flatMap((locale) =>
    DOCS_PAGE_PATHS.map((pagePath) => ({
      url: buildDocsPageUrl(siteUrl, locale, pagePath),
      alternates: {
        languages: buildLanguageAlternates(siteUrl, pagePath)
      }
    }))
  )
}

/**
 * Google이 sitemap ping 엔드포인트를 폐기한 뒤로 robots.txt의 Sitemap 줄이
 * 인증 없이 sitemap을 알릴 수 있는 유일한 경로다.
 */
export function buildDocsRobots(siteUrl) {
  return {
    rules: [
      { userAgent: '*', allow: '/' },
      { userAgent: AI_CRAWLER_USER_AGENTS, allow: '/' }
    ],
    sitemap: `${siteUrl}/sitemap.xml`
  }
}
