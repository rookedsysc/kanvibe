import { generateStaticParamsFor, importPage } from 'nextra/pages'

import SoftwareApplicationStructuredData from '../software-application-structured-data'
import {
  buildDocsPageUrl,
  buildLanguageAlternates,
  resolvePinnedSiteUrl,
  toDocsPagePath
} from '../../../lib/docsSite.mjs'

export const generateStaticParams = generateStaticParamsFor('mdxPath')

const HOME_PAGE_PATH = '/'

/**
 * 문서 페이지는 정적으로 생성되므로 요청 호스트를 알 수 없다.
 * 고정 주소가 정해지기 전에는 canonical과 hreflang을 넣지 않는다 —
 * 틀린 정규 주소를 박는 것보다 태그가 없는 편이 낫고, 언어 변형 신호는
 * Google이 동등하게 인정하는 sitemap의 hreflang이 이미 전달한다.
 */
export async function generateMetadata(props) {
  const params = await props.params
  const { metadata } = await importPage(params.mdxPath, params.lang)
  const pinnedSiteUrl = resolvePinnedSiteUrl()

  if (!pinnedSiteUrl) {
    return metadata
  }

  const pagePath = toDocsPagePath(params.mdxPath)

  return {
    ...metadata,
    alternates: {
      canonical: buildDocsPageUrl(pinnedSiteUrl, params.lang, pagePath),
      languages: buildLanguageAlternates(pinnedSiteUrl, pagePath)
    }
  }
}

export default async function Page(props) {
  const params = await props.params
  const { default: MDXContent } = await importPage(params.mdxPath, params.lang)
  const isHomePage = toDocsPagePath(params.mdxPath) === HOME_PAGE_PATH

  return (
    <>
      {isHomePage && <SoftwareApplicationStructuredData lang={params.lang} />}
      <MDXContent {...props} params={params} />
    </>
  )
}
