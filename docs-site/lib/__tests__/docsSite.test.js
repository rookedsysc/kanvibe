/**
 * sitemap과 robots는 빌드 시점에 파일시스템과 git을 읽는 Node 모듈이라
 * 기본 jsdom 환경 대신 node 환경에서 검증한다.
 *
 * @vitest-environment node
 */

import { createVerify, generateKeyPairSync } from 'node:crypto'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

import { afterEach, describe, expect, it, vi } from 'vitest'

import robots from '../../app/robots.js'
import sitemap from '../../app/sitemap.js'
import { listContentPagePaths } from '../docsContent.mjs'
import {
  DEFAULT_LOCALE,
  DOCS_LOCALES,
  DOCS_PAGE_PATHS,
  buildLanguageAlternates,
  resolveDocsSiteUrl,
  toDocsPagePath
} from '../docsSite.mjs'
import { buildSignedAssertion, buildSitemapSubmitUrl } from '../../scripts/submit-sitemap.mjs'

const SITE_URL = 'https://docs.example.com'

function stubSiteUrl(value = SITE_URL) {
  vi.stubEnv('KANVIBE_DOCS_SITE_URL', value)
}

afterEach(() => {
  vi.unstubAllEnvs()
})

describe('문서 페이지 목록', () => {
  it.each(DOCS_LOCALES)('%s 로케일의 MDX 파일과 DOCS_PAGE_PATHS가 일치한다', (locale) => {
    expect(listContentPagePaths(locale)).toEqual([...DOCS_PAGE_PATHS].sort())
  })
})

describe('resolveDocsSiteUrl', () => {
  it('설정한 주소의 끝 슬래시를 떼어 정규화한다', () => {
    expect(resolveDocsSiteUrl('https://docs.example.com/')).toBe(SITE_URL)
  })

  it('앞뒤 공백이 있어도 읽는다', () => {
    expect(resolveDocsSiteUrl('  https://docs.example.com  ')).toBe(SITE_URL)
  })

  it('절대 URL이 아니면 거부한다', () => {
    expect(() => resolveDocsSiteUrl('docs.example.com')).toThrow(
      /절대 URL/
    )
  })

  it('프로덕션 빌드에서 값이 없으면 빌드를 세운다', () => {
    expect(() => resolveDocsSiteUrl(undefined, 'production')).toThrow(/KANVIBE_DOCS_SITE_URL/)
  })

  it('개발 환경에서는 로컬 주소로 떨어진다', () => {
    expect(resolveDocsSiteUrl(undefined, 'development')).toBe('http://localhost:3000')
  })
})

describe('buildLanguageAlternates', () => {
  it('자기 자신을 포함한 모든 로케일과 x-default를 절대 URL로 만든다', () => {
    expect(buildLanguageAlternates(SITE_URL, '/features')).toEqual({
      ko: 'https://docs.example.com/ko/features',
      en: 'https://docs.example.com/en/features',
      zh: 'https://docs.example.com/zh/features',
      'x-default': 'https://docs.example.com/ko/features'
    })
  })

  it('홈은 로케일 경로만 남긴다', () => {
    expect(buildLanguageAlternates(SITE_URL, '/')['x-default']).toBe('https://docs.example.com/ko')
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

describe('sitemap', () => {
  it('로케일과 페이지의 모든 조합을 담는다', () => {
    stubSiteUrl()

    const entries = sitemap()

    expect(entries).toHaveLength(DOCS_LOCALES.length * DOCS_PAGE_PATHS.length)
    expect(entries.map((entry) => entry.url)).toContain('https://docs.example.com/zh/shortcuts')
  })

  it('각 항목이 자기 자신을 포함한 언어 변형을 가리킨다', () => {
    stubSiteUrl()

    const chineseFeatures = sitemap().find(
      (entry) => entry.url === 'https://docs.example.com/zh/features'
    )

    expect(chineseFeatures.alternates.languages).toEqual({
      ko: 'https://docs.example.com/ko/features',
      en: 'https://docs.example.com/en/features',
      zh: 'https://docs.example.com/zh/features',
      'x-default': 'https://docs.example.com/ko/features'
    })
  })

  it('Google이 무시하는 priority와 changeFrequency는 넣지 않는다', () => {
    stubSiteUrl()

    for (const entry of sitemap()) {
      expect(entry).not.toHaveProperty('priority')
      expect(entry).not.toHaveProperty('changeFrequency')
    }
  })

  it('lastModified를 넣는다면 빌드 시각이 아니라 과거의 커밋 시각이다', () => {
    stubSiteUrl()

    const datedEntries = sitemap().filter((entry) => entry.lastModified)

    for (const entry of datedEntries) {
      expect(entry.lastModified.getTime()).toBeLessThan(Date.now())
    }
  })
})

describe('robots', () => {
  it('sitemap 절대 주소를 알린다', () => {
    stubSiteUrl()

    expect(robots().sitemap).toBe('https://docs.example.com/sitemap.xml')
  })

  it('생성형 검색 크롤러를 허용한다', () => {
    stubSiteUrl()

    const aiRule = robots().rules.find((rule) => Array.isArray(rule.userAgent))

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
