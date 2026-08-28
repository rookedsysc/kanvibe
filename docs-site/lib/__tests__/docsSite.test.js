/**
 * content 디렉터리와 middleware 원문을 파일시스템에서 읽으므로
 * 기본 jsdom 환경 대신 node 환경에서 검증한다.
 *
 * @vitest-environment node
 */

import { createVerify, generateKeyPairSync } from 'node:crypto'
import { readdirSync, readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

import { describe, expect, it } from 'vitest'

import {
  DEFAULT_LOCALE,
  DOCS_LOCALES,
  DOCS_PAGE_PATHS,
  buildDocsRobots,
  buildDocsSitemapEntries,
  buildLanguageAlternates,
  resolvePinnedSiteUrl,
  resolveSiteUrlFromRequestHeaders,
  toDocsPagePath
} from '../docsSite.mjs'
import { buildSignedAssertion, buildSitemapSubmitUrl } from '../../scripts/submit-sitemap.mjs'

const SERVED_SITE_URL = 'https://kanvibe-docs.example.workers.dev'

/** next/headers 대신 실제 요청에서 오는 것과 같은 헤더를 넣어 라우트를 돌린다. */
function stubRequestHeaders(entries) {
  return new Headers(entries)
}

describe('문서 페이지 목록', () => {
  const contentRoot = fileURLToPath(new URL('../../content', import.meta.url))

  it.each(DOCS_LOCALES)('%s 로케일의 MDX 파일과 DOCS_PAGE_PATHS가 일치한다', (locale) => {
    const pagePathsOnDisk = readdirSync(`${contentRoot}/${locale}`, { recursive: true })
      .filter((fileName) => fileName.endsWith('.mdx'))
      .map((fileName) => fileName.replace(/\.mdx$/, ''))
      .map((pagePath) => (pagePath === 'index' ? '/' : `/${pagePath}`))
      .sort()

    expect(pagePathsOnDisk).toEqual([...DOCS_PAGE_PATHS].sort())
  })
})

describe('resolvePinnedSiteUrl', () => {
  it('고정 주소의 끝 슬래시를 떼어 정규화한다', () => {
    expect(resolvePinnedSiteUrl('https://docs.example.com/')).toBe('https://docs.example.com')
  })

  it('앞뒤 공백이 있어도 읽는다', () => {
    expect(resolvePinnedSiteUrl('  https://docs.example.com  ')).toBe('https://docs.example.com')
  })

  it.each([undefined, '', '   '])('%o면 고정 주소가 없다고 본다', (value) => {
    expect(resolvePinnedSiteUrl(value)).toBeUndefined()
  })

  it('스킴이 빠진 값은 변수 이름을 짚어 알려준다', () => {
    expect(() => resolvePinnedSiteUrl('docs.example.com')).toThrow(/KANVIBE_DOCS_SITE_URL/)
  })
})

describe('resolveSiteUrlFromRequestHeaders', () => {
  it('클라이언트가 보낸 x-forwarded-* 대신 실제 Host를 쓴다', () => {
    const siteUrl = resolveSiteUrlFromRequestHeaders(
      stubRequestHeaders({
        host: 'docs.example.com',
        'x-forwarded-host': 'evil.example',
        'x-forwarded-proto': 'http'
      })
    )

    expect(siteUrl).toBe('https://docs.example.com')
  })

  it.each(['a.example.com, b.example.com', '[bad', 'ex ample.com'])(
    '%o처럼 깨진 Host에는 500 대신 아무것도 만들지 않는다',
    (host) => {
      expect(resolveSiteUrlFromRequestHeaders(stubRequestHeaders({ host }))).toBeUndefined()
    }
  )

  it('프로토콜 힌트가 없으면 공개 호스트는 https로 본다', () => {
    expect(resolveSiteUrlFromRequestHeaders(stubRequestHeaders({ host: 'docs.example.com' }))).toBe(
      'https://docs.example.com'
    )
  })

  it('로컬 호스트는 http로 본다', () => {
    expect(resolveSiteUrlFromRequestHeaders(stubRequestHeaders({ host: '127.0.0.1:4321' }))).toBe(
      'http://127.0.0.1:4321'
    )
  })

  it('호스트가 아예 없으면 아무것도 만들지 않는다', () => {
    expect(resolveSiteUrlFromRequestHeaders(stubRequestHeaders({}))).toBeUndefined()
  })
})

describe('buildLanguageAlternates', () => {
  it('자기 자신을 포함한 모든 로케일과 x-default를 절대 URL로 만든다', () => {
    expect(buildLanguageAlternates('https://docs.example.com', '/features')).toEqual({
      ko: 'https://docs.example.com/ko/features',
      en: 'https://docs.example.com/en/features',
      zh: 'https://docs.example.com/zh/features',
      'x-default': 'https://docs.example.com/ko/features'
    })
  })

  it('홈은 로케일 경로만 남긴다', () => {
    expect(buildLanguageAlternates('https://docs.example.com', '/')['x-default']).toBe(
      'https://docs.example.com/ko'
    )
  })
})

describe('toDocsPagePath', () => {
  it.each([
    [undefined, '/'],
    [[], '/'],
    [['features'], '/features']
  ])('%o를 %s로 바꾼다', (segments, expected) => {
    expect(toDocsPagePath(segments)).toBe(expected)
  })
})

describe('buildDocsSitemapEntries', () => {
  it('로케일과 페이지의 모든 조합을 담는다', () => {
    const entries = buildDocsSitemapEntries(SERVED_SITE_URL)

    expect(entries).toHaveLength(DOCS_LOCALES.length * DOCS_PAGE_PATHS.length)
    expect(entries.map((entry) => entry.url)).toContain(`${SERVED_SITE_URL}/zh/shortcuts`)
  })

  it('각 항목이 자기 자신을 포함한 언어 변형을 가리킨다', () => {
    const chineseFeatures = buildDocsSitemapEntries(SERVED_SITE_URL).find(
      (entry) => entry.url === `${SERVED_SITE_URL}/zh/features`
    )

    expect(chineseFeatures.alternates.languages).toEqual({
      ko: `${SERVED_SITE_URL}/ko/features`,
      en: `${SERVED_SITE_URL}/en/features`,
      zh: `${SERVED_SITE_URL}/zh/features`,
      'x-default': `${SERVED_SITE_URL}/ko/features`
    })
  })

  it('출처를 모르면 빈 sitemap을 준다', () => {
    expect(buildDocsSitemapEntries(undefined)).toEqual([])
  })

  it('Google이 무시하거나 신뢰하지 않는 값은 넣지 않는다', () => {
    for (const entry of buildDocsSitemapEntries(SERVED_SITE_URL)) {
      expect(entry).not.toHaveProperty('priority')
      expect(entry).not.toHaveProperty('changeFrequency')
      expect(entry).not.toHaveProperty('lastModified')
    }
  })
})

describe('buildDocsRobots', () => {
  it('sitemap 절대 주소를 알린다', () => {
    expect(buildDocsRobots(SERVED_SITE_URL).sitemap).toBe(`${SERVED_SITE_URL}/sitemap.xml`)
  })

  it('출처를 모르면 Sitemap 줄을 빼고 크롤링 규칙만 준다', () => {
    const robots = buildDocsRobots(undefined)

    expect(robots).not.toHaveProperty('sitemap')
    expect(robots.rules).toHaveLength(2)
  })

  it('생성형 검색 크롤러를 허용한다', () => {
    const aiRule = buildDocsRobots(SERVED_SITE_URL).rules.find((rule) => Array.isArray(rule.userAgent))

    expect(aiRule.allow).toBe('/')
    expect(aiRule.userAgent).toEqual(
      expect.arrayContaining(['OAI-SearchBot', 'Claude-SearchBot', 'PerplexityBot', 'Google-Extended'])
    )
  })
})

describe('middleware matcher', () => {
  const middlewareSource = readFileSync(
    fileURLToPath(new URL('../../middleware.ts', import.meta.url)),
    'utf8'
  )
  const matcherPattern = new RegExp(`^${middlewareSource.match(/'(\/\(\(\?!.+?\)\.\*\))'/)[1]}$`)

  it.each(['/sitemap.xml', '/robots.txt'])('%s는 로케일 미들웨어를 타지 않는다', (path) => {
    expect(matcherPattern.test(path)).toBe(false)
  })

  it.each([`/${DEFAULT_LOCALE}/features`, '/features'])('%s는 로케일 미들웨어를 탄다', (path) => {
    expect(matcherPattern.test(path)).toBe(true)
  })
})

describe('buildSitemapSubmitUrl', () => {
  it('도메인 속성의 콜론을 경로에 안전하게 인코딩한다', () => {
    expect(buildSitemapSubmitUrl('sc-domain:example.com', 'https://docs.example.com/sitemap.xml')).toBe(
      'https://www.googleapis.com/webmasters/v3/sites/sc-domain%3Aexample.com/sitemaps/https%3A%2F%2Fdocs.example.com%2Fsitemap.xml'
    )
  })

  it('URL 접두어 속성의 슬래시를 인코딩한다', () => {
    expect(buildSitemapSubmitUrl('https://docs.example.com/', 'https://docs.example.com/sitemap.xml')).toContain(
      '/sites/https%3A%2F%2Fdocs.example.com%2F/sitemaps/'
    )
  })
})

describe('buildSignedAssertion', () => {
  const { publicKey, privateKey } = generateKeyPairSync('rsa', { modulusLength: 2048 })
  const serviceAccount = {
    client_email: 'docs@example.iam.gserviceaccount.com',
    private_key: privateKey.export({ type: 'pkcs8', format: 'pem' })
  }
  const issuedAtSeconds = 1_700_000_000

  it('서비스 계정 키로 검증되는 RS256 서명을 만든다', () => {
    const [header, claims, signature] = buildSignedAssertion(serviceAccount, issuedAtSeconds).split('.')

    const isSignatureValid = createVerify('RSA-SHA256')
      .update(`${header}.${claims}`)
      .verify(publicKey, Buffer.from(signature, 'base64url'))

    expect(isSignatureValid).toBe(true)
    expect(JSON.parse(Buffer.from(header, 'base64url').toString())).toEqual({ alg: 'RS256', typ: 'JWT' })
  })

  it('Search Console 스코프와 만료 시각을 담는다', () => {
    const claims = JSON.parse(
      Buffer.from(buildSignedAssertion(serviceAccount, issuedAtSeconds).split('.')[1], 'base64url').toString()
    )

    expect(claims).toMatchObject({
      iss: serviceAccount.client_email,
      scope: 'https://www.googleapis.com/auth/webmasters',
      aud: 'https://oauth2.googleapis.com/token',
      iat: issuedAtSeconds
    })
    expect(claims.exp).toBeGreaterThan(claims.iat)
  })
})
