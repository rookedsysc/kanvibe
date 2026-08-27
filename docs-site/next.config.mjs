import { dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import nextra from 'nextra'

import { DEFAULT_LOCALE, DOCS_LOCALES, resolveDocsSiteUrl } from './lib/docsSite.mjs'

const docsRoot = dirname(fileURLToPath(import.meta.url))

const withNextra = nextra({
  defaultShowCopyCode: true,
  contentDirBasePath: '/',
  search: {
    codeblocks: false
  }
})

export default withNextra({
  reactStrictMode: true,
  // sitemap과 robots는 Cloudflare Worker에서 요청 시점에 실행되므로,
  // 검증된 정규 주소를 빌드 시점에 번들로 구워 런타임 환경변수 의존을 없앤다.
  env: {
    KANVIBE_DOCS_SITE_URL: resolveDocsSiteUrl()
  },
  outputFileTracingRoot: docsRoot,
  turbopack: {
    root: docsRoot
  },
  i18n: {
    locales: DOCS_LOCALES,
    defaultLocale: DEFAULT_LOCALE
  }
})
