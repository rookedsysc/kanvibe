import {
  DOCS_LOCALES,
  DOCS_PAGE_PATHS,
  buildDocsPageUrl,
  buildLanguageAlternates,
  resolveDocsSiteUrl
} from '../lib/docsSite.mjs'
import { readContentLastModified } from '../lib/docsContent.mjs'

/**
 * 로케일별 문서 페이지를 모두 나열하고 각 항목에 언어 변형을 붙인다.
 * priority와 changefreq는 Google이 무시한다고 명시했으므로 넣지 않는다.
 */
export default function sitemap() {
  const siteUrl = resolveDocsSiteUrl()

  return DOCS_LOCALES.flatMap((locale) =>
    DOCS_PAGE_PATHS.map((pagePath) => {
      const lastModified = readContentLastModified(locale, pagePath)

      return {
        url: buildDocsPageUrl(siteUrl, locale, pagePath),
        ...(lastModified ? { lastModified } : {}),
        alternates: {
          languages: buildLanguageAlternates(siteUrl, pagePath)
        }
      }
    })
  )
}
