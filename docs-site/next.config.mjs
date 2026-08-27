import { dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import nextra from 'nextra'

import { DEFAULT_LOCALE, DOCS_LOCALES } from './lib/docsSite.mjs'

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
  // sitemap과 robots는 Cloudflare Worker에서 요청 시점에 실행되는데 그 런타임에는
  // 빌드 환경변수가 없다. 고정 주소를 쓰기로 했다면 빌드 시점에 번들로 구워야 한다.
  env: {
    KANVIBE_DOCS_SITE_URL: process.env.KANVIBE_DOCS_SITE_URL ?? ''
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
