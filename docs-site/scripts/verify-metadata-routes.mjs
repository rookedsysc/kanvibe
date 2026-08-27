import { spawn } from 'node:child_process'
import { fileURLToPath } from 'node:url'

/**
 * 빌드된 서버를 띄워 /sitemap.xml과 /robots.txt가 실제로 응답하는지 확인한다.
 * 로케일 미들웨어가 두 경로를 가로채면 크롤러가 찾지 못하는데,
 * 단위 테스트로는 미들웨어 matcher와 라우트 배선을 함께 볼 수 없다.
 */

const DOCS_SITE_ROOT = fileURLToPath(new URL('..', import.meta.url))
const VERIFY_PORT = 4321
const STARTUP_TIMEOUT_MS = 60_000
const POLL_INTERVAL_MS = 500

async function waitForServer(baseUrl) {
  const deadline = Date.now() + STARTUP_TIMEOUT_MS

  while (Date.now() < deadline) {
    try {
      await fetch(baseUrl, { redirect: 'manual' })
      return
    } catch {
      await new Promise((resolve) => setTimeout(resolve, POLL_INTERVAL_MS))
    }
  }

  throw new Error(`${STARTUP_TIMEOUT_MS}ms 안에 문서 서버가 뜨지 않았습니다: ${baseUrl}`)
}

async function assertRouteServes(baseUrl, routePath, expectedFragment) {
  const response = await fetch(`${baseUrl}${routePath}`, { redirect: 'manual' })

  if (response.status !== 200) {
    throw new Error(
      `${routePath}이 200이 아니라 ${response.status}를 돌려줬습니다. ` +
        '로케일 미들웨어가 경로를 가로챘는지 middleware matcher를 확인하세요.'
    )
  }

  const body = await response.text()
  if (!body.includes(expectedFragment)) {
    throw new Error(`${routePath} 응답에 "${expectedFragment}"가 없습니다.`)
  }

  console.log(`${routePath} 200 확인 (${body.length} bytes)`)
}

const baseUrl = `http://127.0.0.1:${VERIFY_PORT}`

const server = spawn('pnpm', ['exec', 'next', 'start', '--port', String(VERIFY_PORT)], {
  cwd: DOCS_SITE_ROOT,
  stdio: ['ignore', 'inherit', 'inherit']
})

try {
  await waitForServer(baseUrl)
  // 고정 주소가 없으면 두 라우트는 요청을 받은 호스트를 그대로 써야 한다.
  const expectedSiteUrl = process.env.KANVIBE_DOCS_SITE_URL?.trim().replace(/\/+$/, '') || baseUrl
  await assertRouteServes(baseUrl, '/sitemap.xml', `<loc>${expectedSiteUrl}/ko</loc>`)
  await assertRouteServes(baseUrl, '/robots.txt', `Sitemap: ${expectedSiteUrl}/sitemap.xml`)
  console.log('메타데이터 라우트 검증 통과')
} finally {
  server.kill('SIGTERM')
}
