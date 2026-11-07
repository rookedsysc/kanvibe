/**
 * @vitest-environment node
 */

import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it, vi } from 'vitest'

import NotFoundPage from '../../app/[lang]/not-found'
import { getDocsDictionary } from '../docsDictionary.mjs'
import { DOCS_LOCALES } from '../docsSite.mjs'

const mockedNavigation = vi.hoisted(() => ({ lang: 'ko' }))

vi.mock('next/navigation', () => ({
  useParams: () => ({ lang: mockedNavigation.lang })
}))

describe('문서 404 페이지', () => {
  it.each(DOCS_LOCALES)('%s 로케일의 안내 문구와 그 로케일의 홈 링크를 보여 준다', (lang) => {
    mockedNavigation.lang = lang
    const { notFound } = getDocsDictionary(lang)

    const markup = renderToStaticMarkup(createElement(NotFoundPage))

    expect(markup).toContain(`<h1>${notFound.title}</h1>`)
    expect(markup).toContain(`href="/${lang}"`)
    expect(markup).toContain(notFound.homeLink)
  })
})
