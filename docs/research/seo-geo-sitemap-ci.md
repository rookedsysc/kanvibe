# Next.js 16 + Nextra 4 다국어 문서 사이트의 SEO/GEO 최적화와 sitemap CI 자동화

KanVibe `docs-site`(Next.js 16 App Router + Nextra 4, ko/en/zh, Cloudflare Workers 배포) 기준으로, Google Search Console 등록에 필요한 sitemap 자동화와 SEO/GEO 최적화 방법을 조사한 결과다.

## 결론 먼저

| 항목 | 판단 | 근거 요약 |
| --- | --- | --- |
| `app/sitemap.ts` + `app/robots.ts` | **적용** | Next.js 기본 규약이라 별도 패키지 불필요, 다국어 `alternates.languages`까지 지원 |
| sitemap ping HTTP 요청 | **적용 안 함** | Google이 폐기했고 지금은 404를 돌려준다 |
| robots.txt `Sitemap:` 줄 | **적용** | Google이 공식으로 권장하는 상시 발견 경로 |
| Search Console API로 CI 제출 | **적용(조건부)** | 서비스 계정을 GSC 속성 소유자로 등록해야 동작 |
| `<priority>` / `<changefreq>` | **적용 안 함** | Google이 명시적으로 무시한다 |
| `<lastmod>` | **적용(정확할 때만)** | 부정확하면 아예 없는 편이 낫다 |
| hreflang + `x-default` | **적용** | 3개 로케일이 겹치는 콘텐츠를 서빙하므로 필수 |
| `llms.txt` | **선택** | 어떤 주요 AI 업체도 공식 지원하지 않음 |
| AI 크롤러 robots.txt 허용 | **적용** | GEO의 전제 조건, Cloudflare 기본 차단 리스크 있음 |
| JSON-LD `SoftwareApplication` / `BreadcrumbList` | **적용** | Google 리치 결과 지원 목록에 있음 |
| JSON-LD `FAQPage` | **적용 안 함** | 리치 결과가 사실상 폐기됨 |

---

## 1. sitemap을 Google에 전달하는 경로

### 1-1. ping 엔드포인트는 죽었다

`https://www.google.com/ping?sitemap=...` 방식의 무인증 제출은 더 이상 동작하지 않는다. CI에서 이 방식을 쓰면 조용히 실패한다.

> [Sitemaps ping endpoint is going away](https://developers.google.com/search/blog/2023/06/sitemaps-lastmod-ping)에서 Google은 폐기 이유로 "unauthenticated sitemap submissions" 방식이 유용하지 않고 "the vast majority of the submissions lead to spam"이라는 점을 든다. 같은 글은 대안으로 "you can still submit your sitemaps through robots.txt and Search Console"이라고 안내한다.

대신 Google이 제시한 대안은 세 가지다: Search Console로 제출, robots.txt에 sitemap 명시, 그리고 실제 변경이 있을 때만 갱신되는 정확한 `lastmod`.

### 1-2. Google이 인정하는 세 가지 제출 방법

> [Build and Submit a Sitemap](https://developers.google.com/search/docs/crawling-indexing/sitemaps/build-sitemap)는 Search Console의 Sitemaps 리포트 제출, robots.txt 명시, Search Console API 세 가지를 든다. robots.txt 방식에 대해서는 "Insert the following line anywhere in your robots.txt file, specifying the path to your sitemap. We will find it the next time we crawl your robots.txt file: `Sitemap: https://example.com/my_sitemap.xml`"라고 안내한다.

즉 **robots.txt 라인은 항상 깔아두고, CI 자동화는 Search Console API로 얹는 구조**가 정석이다. 둘은 배타적이지 않다.

### 1-3. sitemap 태그 중 실제로 쓰이는 것

`priority`와 `changefreq`를 채우는 건 순수한 낭비다.

> 같은 문서에 "Google ignores `<priority>` and `<changefreq>` values."라고 명시되어 있다. 크기 제한은 "All formats limit a single sitemap to 50MB (uncompressed) or 50,000 URLs."이며, `lastmod`에 대해서는 "Google uses the `<lastmod>` value if it's consistently and verifiably accurate"라고 조건을 단다.

`lastmod`는 신뢰성이 전부다. 매 빌드마다 `new Date()`를 넣으면 "모든 페이지가 방금 바뀌었다"는 거짓 신호가 되어 Google이 이 값을 통째로 무시하게 만든다.

> [Sitemap Lastmod: Google Says Drop Inaccurate Dates](https://www.digitalapplied.com/blog/xml-sitemap-lastmod-hygiene-illyes-directive-seo-2026)에 따르면 Google의 Gary Illyes는 lastmod가 부정확한 사이트라면 "probably better off without the lastmods"라고 밝혔다.

Google은 lastmod를 신뢰하거나 통째로 무시하거나 둘 중 하나다.

**docs-site 적용 결과**: `git log -1 --format=%cI <파일>`의 커밋 시각을 쓰려 했으나 실측에서 접었다. OpenNext가 sitemap 라우트를 요청마다 실행하는데 Worker 런타임에는 파일시스템도 git도 없어, 로컬 빌드에만 lastmod가 붙고 배포본에서는 조용히 사라졌다. 정확하지 않을 바에는 넣지 않는 편이 낫다는 위 안내를 그대로 따라 lastmod를 빼기로 했다.

### 1-4. Search Console API 제출 스펙

> [Sitemaps: submit](https://developers.google.com/webmaster-tools/v1/sitemaps/submit)의 요청 형식은 `PUT https://www.googleapis.com/webmasters/v3/sites/siteUrl/sitemaps/feedpath`이며, `siteUrl`은 URL 접두어 형식(`http://www.example.com/`) 또는 도메인 속성 형식(`sc-domain:example.com`)이다. 필요한 OAuth 스코프는 `https://www.googleapis.com/auth/webmasters`이고, 요청 본문은 없으며 성공 시 "this method returns an empty response body"다.

`siteUrl`과 `feedpath`는 모두 URL 인코딩해서 경로에 넣어야 한다.

CI에서 서비스 계정으로 호출할 때의 가장 흔한 실패는 권한이다.

서비스 계정 이메일(키 JSON의 `client_email`, 보통 `name@project-id.iam.gserviceaccount.com`)을 해당 GSC 속성의 사용자로 추가하지 않으면 403이 난다. 개인 Google 계정이 아니라 이 이메일을 등록해야 한다.

> [Managing owners, users, and permissions](https://support.google.com/webmasters/answer/7687615)에 따르면 Owner는 "Add and remove other users, configure settings, view all data, and use all tools" 권한을 갖는다. 같은 문서의 권한 표에서 sitemap 제출은 Owner와 "Full user"에게 허용되고 Restricted user에게는 허용되지 않는다.

따라서 최소 요건은 Full user이고, 이후 Indexing API 같은 소유자 전용 기능까지 열어둘 생각이라면 Owner로 등록하는 편이 단순하다.

**docs-site 적용안**: GitHub Actions에서 `GOOGLE_SEARCH_CONSOLE_CREDENTIALS`(서비스 계정 키 JSON) 시크릿이 있을 때만 제출 단계를 돌리고, 없으면 건너뛴다. 시크릿이 없다고 CI를 깨뜨리면 포크 PR과 초기 설정 전 기간에 계속 실패한다.

---

## 2. Next.js App Router의 sitemap/robots 규약

### 2-1. `app/sitemap.ts`

`sitemap.(xml|js|ts)`는 App Router의 특수 파일이고, 기본 함수가 URL 배열을 반환하면 Next.js가 XML로 직렬화한다.

```ts
import type { MetadataRoute } from 'next'

export default function sitemap(): MetadataRoute.Sitemap {
  return [
    {
      url: 'https://acme.com',
      lastModified: new Date(),
      changeFrequency: 'yearly',
      priority: 1,
    },
  ]
}
```
> 출처: [Next.js sitemap.xml API Reference](https://nextjs.org/docs/app/api-reference/file-conventions/metadata/sitemap) (v16.3.3). 같은 문서에 "`sitemap.js` is a special Route Handler that is cached by default unless it uses a Request-time API or dynamic config option"이라고 명시되어 있다.

반환 타입은 다음과 같다.

```tsx
type Sitemap = Array<{
  url: string
  lastModified?: string | Date
  changeFrequency?:
    | 'always'
    | 'hourly'
    | 'daily'
    | 'weekly'
    | 'monthly'
    | 'yearly'
    | 'never'
  priority?: number
  alternates?: {
    languages?: Languages<string>
  }
  images?: string[]
  videos?: Videos[]
}>
```
> 출처: 위와 같은 [Next.js sitemap.xml API Reference](https://nextjs.org/docs/app/api-reference/file-conventions/metadata/sitemap)의 "Returns" 절

### 2-2. 다국어 sitemap은 `alternates.languages`로

로케일별 URL을 따로 나열하는 대신, 대표 URL 하나에 `alternates.languages`를 달면 Next.js가 `xhtml:link` 요소로 풀어준다.

```ts
import type { MetadataRoute } from 'next'

export default function sitemap(): MetadataRoute.Sitemap {
  return [
    {
      url: 'https://acme.com',
      lastModified: new Date(),
      alternates: {
        languages: {
          es: 'https://acme.com/es',
          de: 'https://acme.com/de',
        },
      },
    },
  ]
}
```
> 출처: [Next.js sitemap.xml API Reference](https://nextjs.org/docs/app/api-reference/file-conventions/metadata/sitemap)의 "Generate a localized Sitemap" 절. 이 입력은 `<urlset ... xmlns:xhtml="http://www.w3.org/1999/xhtml">` 안에서 `<xhtml:link rel="alternate" hreflang="es" href="https://acme.com/es"/>` 형태로 출력된다. 로컬라이제이션 지원은 v14.2.0에 추가됐다.

버전 히스토리상 `changeFrequency`/`priority`는 v13.4.14, localization은 v14.2.0에 들어왔으므로 현재 `next@^16.2.6`에서는 모두 사용 가능하다.

### 2-3. `app/robots.ts`

```ts
import type { MetadataRoute } from 'next'

export default function robots(): MetadataRoute.Robots {
  return {
    rules: {
      userAgent: '*',
      allow: '/',
      disallow: '/private/',
    },
    sitemap: 'https://acme.com/sitemap.xml',
  }
}
```
> 출처: [Next.js robots.txt API Reference](https://nextjs.org/docs/app/api-reference/file-conventions/metadata/robots). 같은 문서는 "`robots.js` is a special Route Handler that is cached by default unless it uses a Request-time API or dynamic config option"이라고 밝힌다.

`rules`에 배열을 넘기면 user-agent별로 블록이 나뉜다. 타입은 다음과 같다.

```tsx
type Robots = {
  rules:
    | {
        userAgent?: string | string[]
        allow?: string | string[]
        disallow?: string | string[]
        crawlDelay?: number
        other?: Record<string, string | number | Array<string | number>>
      }
    | Array<{
        userAgent: string | string[]
        allow?: string | string[]
        disallow?: string | string[]
        crawlDelay?: number
        other?: Record<string, string | number | Array<string | number>>
      }>
  sitemap?: string | string[]
  host?: string
}
```
> 출처: 위와 같은 [Next.js robots.txt API Reference](https://nextjs.org/docs/app/api-reference/file-conventions/metadata/robots)의 "Robots object" 절. `other` 필드는 v16.3.0에 추가된 비표준 디렉티브 통로다.

### 2-4. middleware matcher 주의

`docs-site/middleware.ts`의 matcher는 현재 `'/((?!api|_next/static|_next/image|favicon.ico|icon.png|apple-icon.png|manifest|screenshots).*)'`이다. Nextra의 로케일 미들웨어가 `/sitemap.xml`과 `/robots.txt` 요청까지 잡아 `/ko/sitemap.xml`로 리다이렉트하면 Google이 sitemap을 찾지 못한다. **matcher 제외 목록에 `sitemap.xml`, `robots.txt`를 추가해야 한다.**

---

## 3. hreflang 다국어 규칙

> [Localized versions of your page](https://developers.google.com/search/docs/specialty/international/localized-versions)에서 Google은 HTML 태그, HTTP 헤더, Sitemap 세 가지 방법을 동등하게 지원한다고 밝히고 "choose the method that's the most convenient for your site"라고 안내한다.

핵심 제약은 상호성이다.

> 같은 문서에 "If two pages don't both point to each other, the tags will be ignored."라고 명시되어 있다. 즉 ko 페이지가 en/zh를 가리키면 en/zh 페이지도 ko를 포함해 모든 변형을 가리켜야 하며, **각 페이지는 자기 자신도 목록에 포함해야 한다.**

`x-default`는 어느 로케일에도 매칭되지 않는 사용자를 위한 폴백이다.

> Google은 `x-default`가 "language selector pages"에서 "will work best"하며 "users whose language settings don't match any of your site's localized versions"에 적용된다고 설명한다.

언어 코드는 ISO 639-1(+선택적으로 ISO 3166-1 Alpha 2 지역 코드)이며, 지역 코드만 단독으로 쓸 수 없다. docs-site의 `ko`/`en`/`zh`는 유효한 값이다.

Next.js 쪽 대응은 `generateMetadata`의 `alternates`다.

```jsx
export const metadata = {
  alternates: {
    canonical: 'https://nextjs.org',
    languages: {
      'en-US': 'https://nextjs.org/en-US',
      'de-DE': 'https://nextjs.org/de-DE',
    },
  },
}
```
> 출처: [Next.js generateMetadata 레퍼런스](https://nextjs.org/docs/app/api-reference/functions/generate-metadata)의 `alternates` 절. 이 입력은 `<link rel="canonical" href="..."/>`와 `<link rel="alternate" hreflang="en-US" href="..."/>`로 출력된다.

절대 URL의 기준점은 `metadataBase`다.

> 같은 문서는 "`metadataBase` is a convenient option to set a base URL prefix for `metadata` fields that require a fully qualified URL"이며 "typically set in root `app/layout.js`"라고 설명한다. 또한 "If a `metadata` field provides an absolute URL, `metadataBase` will be ignored."라고 밝힌다.

**docs-site 현황 문제**: `app/[lang]/layout.jsx`의 `metadataBase`가 `new URL('https://github.com/rookedsysc/kanvibe')`로 잡혀 있다. 이건 문서 사이트의 정규 주소가 아니라 소스 저장소 주소라서, OG 이미지·canonical 등 상대 경로 기반 필드가 전부 GitHub 도메인으로 해석된다. 문서 사이트 실제 도메인으로 교정해야 한다.

---

## 4. GEO(Generative Engine Optimization)

### 4-1. Google 공식 입장: 별도 최적화는 없다

> [AI features and your website](https://developers.google.com/search/docs/appearance/ai-features)에서 Google은 "There are no additional requirements to appear in AI Overviews or AI Mode, nor other special optimizations necessary."라고 못박는다. 나아가 "You don't need to create new machine readable files, AI text files, or markup to appear in these features."라고 명시한다.

Google이 실제로 요구하는 것은 기존 SEO 기본기 — 크롤 가능성, 유용하고 신뢰할 수 있는 콘텐츠, 텍스트 기반 페이지 경험 — 그대로다. 노출 제어 수단으로는 `nosnippet`, `data-nosnippet`, `max-snippet`, `noindex`를 든다.

### 4-2. `llms.txt`는 "해도 되지만 근거는 약함"

Google 쪽 1차 근거는 4-1에서 인용한 문장 그대로다. "You don't need to create new machine readable files, AI text files, or markup to appear in these features."의 "AI text files"가 정확히 llms.txt 같은 파일을 가리킨다.

> 출처: [AI features and your website](https://developers.google.com/search/docs/appearance/ai-features)

업계 조사도 같은 방향이다.

> [State of llms.txt 2026](https://presenc.ai/research/state-of-llms-txt-2026)에 따르면 2026년 1분기 기준 OpenAI, Google, Anthropic, Meta, Mistral을 포함한 어떤 주요 AI 기업도 프로덕션 시스템에서 llms.txt를 읽거나 반영한다고 공개적으로 밝힌 적이 없다. SE Ranking의 30만 도메인 조사 기준 채택률은 10.13% 수준이다.

**판단**: docs-site 콘텐츠가 6페이지 × 3언어로 작아 llms.txt 유지 비용이 거의 없으므로 넣어도 손해는 아니다. 다만 검색 노출 효과로 포장하면 안 되고, 효용이 입증되지 않은 선택 항목으로 다뤄야 한다. 우선순위는 robots.txt와 구조화 데이터보다 확실히 낮다.

### 4-3. AI 크롤러 허용이 GEO의 실질적 전제

llms.txt보다 훨씬 중요한 건 AI 크롤러가 robots.txt에서 막히지 않는 것이다.

OpenAI 크롤러:

> [OpenAI bots 문서](https://developers.openai.com/api/docs/bots)는 `OAI-SearchBot`이 "used to surface websites in search results in ChatGPT's search features", `GPTBot`이 "used to crawl content that may be used in training our generative AI foundation models", `ChatGPT-User`가 ChatGPT와 Custom GPT에서 사용자 행동에 의해 페이지를 방문할 때 쓰인다고 설명한다.

Anthropic 크롤러:

> [Anthropic 크롤러 안내](https://support.claude.com/en/articles/8896518-does-anthropic-crawl-data-from-the-web-and-how-can-site-owners-block-the-crawler)는 `ClaudeBot`이 학습용 수집("collecting web content that could potentially contribute to their training"), `Claude-User`가 사용자 질문 시 접근("When individuals ask questions to Claude, it may access websites using a Claude-User agent"), `Claude-SearchBot`이 검색 품질 개선("navigates the web to improve search result quality for users")이라고 구분한다.

Google의 AI 전용 토큰:

> [Google common crawlers](https://developers.google.com/search/docs/crawling-indexing/google-common-crawlers)에 따르면 `Google-Extended`는 Gemini 모델 학습과 그라운딩 사용 여부를 제어하며, "Google-Extended does not impact a site's inclusion in Google Search nor is it used as a ranking signal in Google Search."다.

정리하면 **검색 노출을 원한다면 최소한 `OAI-SearchBot`, `Claude-SearchBot`, `PerplexityBot`은 허용해야 하고, 학습용 수집(`GPTBot`, `ClaudeBot`, `Google-Extended`)은 정책적으로 따로 정할 수 있다.** 오픈소스 문서 사이트라면 전부 허용하는 편이 노출에 유리하다.

### 4-4. Cloudflare 기본 차단 리스크 — docs-site에 직접 해당

docs-site는 Cloudflare Workers에 배포된다.

Cloudflare는 2025년 7월 AI 크롤러 기본 차단과 Pay Per Crawl을 발표했고, 그 정책은 이후 크롤러 목적별로 세분화됐다.

> [Cloudflare Just Changed How AI Crawlers Scrape the Internet-at-Large](https://www.cloudflare.com/press/press-releases/2025/cloudflare-just-changed-how-ai-crawlers-scrape-the-internet-at-large/)는 AI 크롤러 접근을 허가 기반으로 바꾸고 "Pay Per Crawl" 마켓플레이스를 함께 도입한다고 발표했다.

> [Your site, your rules: new AI traffic options for all customers](https://blog.cloudflare.com/content-independence-day-ai-options/)에 따르면 2026년 9월 15일부터 "all new domains onboarding to Cloudflare"에 새 기본값이 적용된다. 광고를 노출하는 페이지에서는 "Training and Agent will be blocked by default on the pages that display ads, while Search will remain allowed by default"이며, 설정은 대시보드의 "Security settings"에서 Free 플랜을 포함한 모든 고객이 바꿀 수 있다.

docs-site는 광고가 없으므로 이 기본값에 그대로 걸릴 가능성은 낮다. 하지만 **robots.txt에서 아무리 허용해도 Cloudflare 존 설정이 AI 크롤러를 막고 있으면 무의미하다는 구조는 그대로다.** 코드로 해결할 수 없는 대시보드 설정이므로, 배포 존의 AI 크롤러 설정을 눈으로 확인하는 운영 항목으로 남긴다.

### 4-5. 콘텐츠 구조

Google이 AI 노출을 위해 실제로 요구하는 것은 별도 기법이 아니라 기존 품질 기준이다.

> [AI features and your website](https://developers.google.com/search/docs/appearance/ai-features)는 "Creating helpful, reliable, people-first content"를 핵심으로 두고, robots.txt와 호스팅 측면의 크롤 가능성, 좋은 페이지 경험, 텍스트 기반 콘텐츠를 함께 든다.

업계 GEO 가이드가 덧붙이는 실행 항목 — 명확한 헤딩 계층, 짧은 문단과 불릿, 질문에 대한 직접적 답변, Breadcrumb 등 스키마 — 은 위 기준을 문서 형식으로 옮긴 것에 가깝고, Google이 별도로 요구하는 바는 아니다.

docs-site MDX는 이미 헤딩과 표 중심이라 구조 자체는 양호하다. 부족한 건 페이지별 `description`이다.

---

## 5. JSON-LD 구조화 데이터

> [Structured Data Markup that Google Search Supports](https://developers.google.com/search/docs/appearance/structured-data/search-gallery) 목록에는 **Software App**("Information about a software app, including rating information, a description of the app, and a link to the app"), **Article**, **Breadcrumb**("Navigation that indicates the page's position in the site hierarchy")이 포함된다. 반면 FAQ는 독립 항목으로 존재하지 않는다.

FAQ는 명시적으로 정리된 상태다.

> [FAQPage 문서](https://developers.google.com/search/docs/appearance/structured-data/faqpage)에 따르면 FAQ 리치 결과는 2023년 9월에 폐기되어 지금은 "well-known, authoritative government and health websites"에만 노출되며, FAQPage 마크업 문서 자체가 2026년 6월에 제거됐다. 넣어도 해롭지는 않지만 Google Search 노출에 이득이 없다.

> [Software App (SoftwareApplication) Schema](https://developers.google.com/search/docs/appearance/structured-data/software-app)는 `VideoGame` 타입만 단독으로 쓰면 리치 결과가 나오지 않으므로 다른 타입과 co-type 하라고 안내한다. 일반 데스크톱 앱인 KanVibe에는 해당하지 않는다.

**docs-site 적용안**: 홈에 `SoftwareApplication`(`applicationCategory: DeveloperApplication`, `operatingSystem`, `offers`), 전 페이지에 `BreadcrumbList` 정도가 근거 있는 선택이다. `TechArticle`은 Article 계열이라 문서 페이지에 쓸 수 있지만, 리치 결과는 뉴스/블로그 성격에 맞춰져 있어 효과는 제한적이다. FAQPage는 넣지 않는다.

---

## 6. Cloudflare Workers(OpenNext)에서의 동작

> [OpenNext Cloudflare 문서](https://opennext.js.org/cloudflare)의 지원 목록에는 App Router, "Route Handlers", SSG, SSR, 동적 라우트, ISR, PPR, Composable Caching(`'use cache'`), 미들웨어, `after()`가 있다. 명시적 미지원으로는 15.2에서 도입된 Node Middleware를 든다.

`sitemap.ts`와 `robots.ts`는 2-1·2-3에서 인용했듯 Next.js가 특수 Route Handler로 규정하는 파일이므로, 위 목록에 있는 Route Handlers 지원 범위 안에 들어간다.

**다만 실측에서 드러난 중요한 차이가 있다.** Next 빌드 로그가 두 라우트를 `○ (Static)`으로 표시해도 **OpenNext는 정적 자산으로 서빙하지 않고 서버 함수로 태운다.** 즉 요청마다 라우트 코드가 다시 실행되며, 그 런타임에는 빌드 환경변수도 파일시스템도 git도 없다. 빌드 시점에만 얻을 수 있는 값에 의존하면 로컬에서는 멀쩡하고 배포본에서만 값이 비거나 500이 난다. 이 두 라우트는 번들에 구워진 데이터나 요청 헤더만으로 동작하도록 짜야 한다. 다만 문서에 metadata route가 개별 항목으로 명시되어 있지는 않으므로, **`pnpm build:worker` 후 `wrangler dev`로 `/sitemap.xml`과 `/robots.txt`가 200과 올바른 Content-Type을 반환하는지 실제 확인이 필요하다.**

빌드 시점에 정적으로 생성되게 하려면 `sitemap.ts`가 요청 시점 API(`headers()`, `cookies()` 등)를 쓰지 않아야 한다. 파일시스템/`git log` 기반 `lastmod`는 빌드 타임에 해결되므로 이 조건을 만족한다.

---

## 7. docs-site 현황 대비 갭 정리

| 갭 | 현재 | 필요한 조치 |
| --- | --- | --- |
| sitemap 없음 | `app/`에 `sitemap.ts` 부재 | `app/sitemap.ts` 추가, 로케일 `alternates` 포함 |
| robots.txt 없음 | `app/`에 `robots.ts` 부재, `public/`에도 없음 | `app/robots.ts` 추가 + `Sitemap:` 라인 |
| `metadataBase` 오설정 | `https://github.com/rookedsysc/kanvibe` | 문서 사이트 정규 도메인으로 교정 |
| hreflang 없음 | `alternates` 미설정 | 페이지 metadata에 `languages` + `x-default` |
| canonical 없음 | 미설정 | 로케일별 자기 URL을 canonical로 |
| 페이지 description 없음 | MDX frontmatter에 `title`만 있음 | 각 MDX에 `description` 추가 |
| middleware가 sitemap을 가로챌 위험 | matcher 제외 목록에 없음 | `sitemap.xml`, `robots.txt` 제외 추가 |
| 구조화 데이터 없음 | JSON-LD 부재 | `SoftwareApplication` + `BreadcrumbList` |
| docs CI 없음 | `.github/workflows/ci.yml`은 데스크톱 앱만 검사 | docs 빌드 검증 + sitemap 제출 워크플로 |

Nextra 4는 frontmatter의 `title`/`description`을 Next.js Metadata API로 넘긴다.

> [Nextra 4 x App Router](https://the-guild.dev/blog/nextra-4)는 기존 `head` 테마 설정 대신 "use Next.js Metadata API instead"라고 안내한다. frontmatter의 `title`은 title/og:title을, `description`은 description/og:description을 설정한다.

따라서 각 MDX에 `description`을 추가하는 것만으로 메타 설명과 OG 설명이 함께 채워진다.

---

## 8. 아직 확정되지 않은 값

이 조사만으로 정해지지 않고 사용자 결정이 필요한 항목이다.

- **문서 사이트의 정규 도메인.** `wrangler.jsonc`에 `routes`나 커스텀 도메인 설정이 없어 현재는 `kanvibe-docs.<account>.workers.dev`로 서빙될 가능성이 높다. sitemap `loc`, canonical, hreflang, robots `Sitemap:` 모두 이 값에 의존하므로 확정이 선행되어야 한다.
- **Search Console 속성 유형.** 도메인 속성(`sc-domain:`)인지 URL 접두어 속성인지에 따라 API `siteUrl` 값이 달라진다.
- **학습용 AI 크롤러 정책.** `GPTBot`/`ClaudeBot`/`Google-Extended`를 허용할지 여부는 기술 판단이 아니라 정책 판단이다.
