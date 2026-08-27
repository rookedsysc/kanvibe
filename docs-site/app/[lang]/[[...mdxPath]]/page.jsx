import { generateStaticParamsFor, importPage } from 'nextra/pages'

import SoftwareApplicationStructuredData from '../software-application-structured-data'
import {
  buildDocsPageUrl,
  buildLanguageAlternates,
  resolveDocsSiteUrl,
  toDocsPagePath
} from '../../../lib/docsSite.mjs'

export const generateStaticParams = generateStaticParamsFor('mdxPath')

const HOME_PAGE_PATH = '/'

export async function generateMetadata(props) {
  const params = await props.params
  const { metadata } = await importPage(params.mdxPath, params.lang)
  const siteUrl = resolveDocsSiteUrl()
  const pagePath = toDocsPagePath(params.mdxPath)

  return {
    ...metadata,
    alternates: {
      canonical: buildDocsPageUrl(siteUrl, params.lang, pagePath),
      languages: buildLanguageAlternates(siteUrl, pagePath)
    }
  }
}

export default async function Page(props) {
  const params = await props.params
  const { default: MDXContent } = await importPage(params.mdxPath, params.lang)
  const isHomePage = toDocsPagePath(params.mdxPath) === HOME_PAGE_PATH

  return (
    <>
      {isHomePage && (
        <SoftwareApplicationStructuredData
          lang={params.lang}
          homeUrl={buildDocsPageUrl(resolveDocsSiteUrl(), params.lang, HOME_PAGE_PATH)}
        />
      )}
      <MDXContent {...props} params={params} />
    </>
  )
}
