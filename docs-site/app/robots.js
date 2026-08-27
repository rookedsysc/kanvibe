import { buildDocsRobots } from '../lib/docsSite.mjs'
import { resolveDocsSiteUrl } from './docsSiteUrl.js'

export default async function robots() {
  return buildDocsRobots(await resolveDocsSiteUrl())
}
