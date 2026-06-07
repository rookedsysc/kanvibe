import { generateStaticParamsFor, importPage } from 'nextra/pages'

export const generateStaticParams = generateStaticParamsFor('mdxPath')

export async function generateMetadata(props) {
  const params = await props.params
  const { metadata } = await importPage(params.mdxPath, params.lang)
  return metadata
}

export default async function Page(props) {
  const params = await props.params
  const { default: MDXContent } = await importPage(params.mdxPath, params.lang)
  return <MDXContent {...props} params={params} />
}
