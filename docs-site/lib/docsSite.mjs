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

const DEVELOPMENT_SITE_URL = 'http://localhost:3000'

const MISSING_SITE_URL_MESSAGE =
  'KANVIBE_DOCS_SITE_URL이 필요합니다. sitemap의 loc과 canonical, hreflang은 문서 사이트의 정규 주소 없이는 만들 수 없습니다. ' +
  '예: KANVIBE_DOCS_SITE_URL=https://docs.example.com pnpm build'

function normalizeSiteUrl(rawSiteUrl) {
  let parsedSiteUrl
  try {
    parsedSiteUrl = new URL(rawSiteUrl)
  } catch {
    throw new Error(`KANVIBE_DOCS_SITE_URL은 절대 URL이어야 합니다: ${rawSiteUrl}`)
  }

  if (parsedSiteUrl.protocol !== 'https:' && parsedSiteUrl.protocol !== 'http:') {
    throw new Error(`KANVIBE_DOCS_SITE_URL은 http 또는 https여야 합니다: ${rawSiteUrl}`)
  }

  const basePath = parsedSiteUrl.pathname.replace(/\/+$/, '')
  return `${parsedSiteUrl.origin}${basePath}`
}

/**
 * 문서 사이트의 정규 주소를 돌려준다.
 * 프로덕션 빌드에서 값이 없으면 잘못된 canonical을 조용히 배포하는 대신 빌드를 세운다.
 *
 * 기본값을 `process.env` 멤버 접근 그대로 두는 것이 중요하다.
 * next.config의 `env`가 이 참조를 빌드 시점 값으로 치환하기 때문에,
 * Cloudflare Worker 런타임에는 환경변수 없이도 주소가 남는다.
 */
export function resolveDocsSiteUrl(
  configuredSiteUrl = process.env.KANVIBE_DOCS_SITE_URL,
  nodeEnv = process.env.NODE_ENV
) {
  const trimmedSiteUrl = configuredSiteUrl?.trim()
  if (trimmedSiteUrl) {
    return normalizeSiteUrl(trimmedSiteUrl)
  }

  if (nodeEnv === 'production') {
    throw new Error(MISSING_SITE_URL_MESSAGE)
  }

  return DEVELOPMENT_SITE_URL
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
