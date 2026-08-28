import { buildDocsSitemapEntries } from '../lib/docsSite.mjs'
import { resolveDocsSiteUrl } from './docsSiteUrl.js'

export default async function sitemap() {
  return buildDocsSitemapEntries(await resolveDocsSiteUrl())
}
