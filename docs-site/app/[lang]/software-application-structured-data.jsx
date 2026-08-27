import { getDocsDictionary } from '../../lib/docsDictionary.mjs'

const KANVIBE_REPOSITORY_URL = 'https://github.com/rookedsysc/kanvibe'
const KANVIBE_LICENSE_URL = `${KANVIBE_REPOSITORY_URL}/blob/main/LICENSE`

/**
 * 홈 문서에 KanVibe 앱 자체를 설명하는 구조화 데이터를 심는다.
 * Google이 Software App 리치 결과를 뽑으려면 offers 또는 평점이 필요해서
 * AGPL-3.0으로 무료 배포된다는 사실을 offers로 표현한다.
 */
export default function SoftwareApplicationStructuredData({ lang, homeUrl }) {
  const structuredData = {
    '@context': 'https://schema.org',
    '@type': 'SoftwareApplication',
    name: 'KanVibe',
    description: getDocsDictionary(lang).tagline,
    applicationCategory: 'DeveloperApplication',
    operatingSystem: 'macOS',
    url: homeUrl,
    softwareHelp: homeUrl,
    codeRepository: KANVIBE_REPOSITORY_URL,
    license: KANVIBE_LICENSE_URL,
    inLanguage: lang,
    offers: {
      '@type': 'Offer',
      price: '0',
      priceCurrency: 'USD'
    }
  }

  return (
    <script
      type="application/ld+json"
      dangerouslySetInnerHTML={{ __html: JSON.stringify(structuredData) }}
    />
  )
}
