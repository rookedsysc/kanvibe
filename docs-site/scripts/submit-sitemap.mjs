import { createSign } from 'node:crypto'
import { pathToFileURL } from 'node:url'

import { resolvePinnedSiteUrl } from '../lib/docsSite.mjs'

/**
 * Search Console에 sitemap을 제출한다.
 * Google이 무인증 ping 엔드포인트를 폐기했기 때문에 CI에서 자동 제출하려면
 * 서비스 계정으로 인증한 Search Console API를 써야 한다.
 */

const GOOGLE_TOKEN_URL = 'https://oauth2.googleapis.com/token'
const SEARCH_CONSOLE_SCOPE = 'https://www.googleapis.com/auth/webmasters'
const JWT_BEARER_GRANT_TYPE = 'urn:ietf:params:oauth:grant-type:jwt-bearer'
const ASSERTION_LIFETIME_SECONDS = 3600

function toBase64Url(value) {
  return Buffer.from(value).toString('base64url')
}

/**
 * siteUrl과 sitemap 주소는 경로 세그먼트로 들어가므로 콜론과 슬래시까지 인코딩해야 한다.
 * 도메인 속성은 `sc-domain:example.com` 형태라 콜론이 그대로 남으면 경로가 깨진다.
 */
export function buildSitemapSubmitUrl(searchConsoleSiteUrl, sitemapUrl) {
  const encodedSite = encodeURIComponent(searchConsoleSiteUrl)
  const encodedSitemap = encodeURIComponent(sitemapUrl)
  return `https://www.googleapis.com/webmasters/v3/sites/${encodedSite}/sitemaps/${encodedSitemap}`
}

export function buildSignedAssertion(credentials, issuedAtSeconds) {
  const header = { alg: 'RS256', typ: 'JWT' }
  const claims = {
    iss: credentials.client_email,
    scope: SEARCH_CONSOLE_SCOPE,
    aud: GOOGLE_TOKEN_URL,
    iat: issuedAtSeconds,
    exp: issuedAtSeconds + ASSERTION_LIFETIME_SECONDS
  }

  const signingInput = `${toBase64Url(JSON.stringify(header))}.${toBase64Url(JSON.stringify(claims))}`
  const signature = createSign('RSA-SHA256').update(signingInput).sign(credentials.private_key)

  return `${signingInput}.${signature.toString('base64url')}`
}

async function requestAccessToken(credentials) {
  const assertion = buildSignedAssertion(credentials, Math.floor(Date.now() / 1000))

  const response = await fetch(GOOGLE_TOKEN_URL, {
    method: 'POST',
    headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
    body: new URLSearchParams({ grant_type: JWT_BEARER_GRANT_TYPE, assertion })
  })

  if (!response.ok) {
    throw new Error(`액세스 토큰 발급 실패 (${response.status}): ${await response.text()}`)
  }

  return (await response.json()).access_token
}

async function main() {
  const rawCredentials = process.env.KANVIBE_GSC_CREDENTIALS?.trim()
  const searchConsoleSiteUrl = process.env.KANVIBE_GSC_SITE_URL?.trim()

  if (!rawCredentials || !searchConsoleSiteUrl) {
    console.log(
      'KANVIBE_GSC_CREDENTIALS 또는 KANVIBE_GSC_SITE_URL이 없어 sitemap 제출을 건너뜁니다. ' +
        '서비스 계정을 Search Console 속성 사용자로 추가한 뒤 두 값을 설정하세요.'
    )
    return
  }

  // 라우트와 달리 이 스크립트에는 요청 컨텍스트가 없어 호스트를 유추할 길이 없다.
  // 고정 주소가 없으면 조용히 "undefined/sitemap.xml"을 제출하게 되므로 여기서 멈춘다.
  const docsSiteUrl = resolvePinnedSiteUrl()
  if (!docsSiteUrl) {
    throw new Error(
      'KANVIBE_DOCS_SITE_URL이 없어 제출할 sitemap 주소를 정할 수 없습니다. ' +
        'Search Console 제출을 쓰려면 문서 사이트의 정규 주소를 변수로 설정하세요.'
    )
  }

  const sitemapUrl = `${docsSiteUrl}/sitemap.xml`
  const accessToken = await requestAccessToken(JSON.parse(rawCredentials))

  const response = await fetch(buildSitemapSubmitUrl(searchConsoleSiteUrl, sitemapUrl), {
    method: 'PUT',
    headers: { Authorization: `Bearer ${accessToken}` }
  })

  if (!response.ok) {
    throw new Error(
      `sitemap 제출 실패 (${response.status}): ${await response.text()}\n` +
        '403이면 서비스 계정이 해당 Search Console 속성의 사용자로 등록되지 않은 경우가 대부분입니다.'
    )
  }

  console.log(`sitemap 제출 완료: ${sitemapUrl} -> ${searchConsoleSiteUrl}`)
}

const isDirectRun =
  process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href

if (isDirectRun) {
  await main()
}
