import { execFileSync } from 'node:child_process'
import { readdirSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

/**
 * content 디렉터리의 MDX 파일을 읽어 문서 페이지 목록과 마지막 변경 시각을 구한다.
 * 빌드 시점에만 호출되며, Cloudflare Worker 런타임에서는 실행되지 않는다.
 */

// webpack은 `new URL(<literal>, import.meta.url)`을 모듈 참조로 해석해 번들에 넣으려 하므로
// 경로는 path 조합으로 만든다.
const DOCS_SITE_ROOT = dirname(dirname(fileURLToPath(import.meta.url)))
const CONTENT_ROOT = join(DOCS_SITE_ROOT, 'content')

const INDEX_PAGE_FILE = 'index.mdx'

function toContentFileName(pagePath) {
  return pagePath === '/' ? INDEX_PAGE_FILE : `${pagePath.slice(1)}.mdx`
}

/** content/<locale> 아래에 실제로 존재하는 문서 페이지 경로를 정렬해 돌려준다. */
export function listContentPagePaths(locale) {
  return readdirSync(join(CONTENT_ROOT, locale))
    .filter((fileName) => fileName.endsWith('.mdx'))
    .map((fileName) => (fileName === INDEX_PAGE_FILE ? '/' : `/${fileName.replace(/\.mdx$/, '')}`))
    .sort()
}

/**
 * 해당 MDX 파일의 마지막 커밋 시각을 돌려준다.
 * Google은 정확한 lastmod만 신뢰하고 부정확하면 통째로 무시하므로,
 * git 이력을 읽을 수 없으면 빌드 시각으로 대신하지 않고 undefined를 돌려준다.
 */
export function readContentLastModified(locale, pagePath) {
  const contentFilePath = `content/${locale}/${toContentFileName(pagePath)}`

  let lastCommittedAt
  try {
    lastCommittedAt = execFileSync(
      'git',
      ['log', '-1', '--format=%cI', '--', contentFilePath],
      { cwd: DOCS_SITE_ROOT, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }
    ).trim()
  } catch {
    return undefined
  }

  return lastCommittedAt ? new Date(lastCommittedAt) : undefined
}
