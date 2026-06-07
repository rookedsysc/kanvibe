import { dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import nextra from 'nextra'

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
  outputFileTracingRoot: docsRoot,
  turbopack: {
    root: docsRoot
  },
  i18n: {
    locales: ['ko', 'en', 'zh'],
    defaultLocale: 'ko'
  }
})
