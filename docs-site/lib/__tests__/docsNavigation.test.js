/**
 * @vitest-environment node
 */

import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it, vi } from 'vitest'

import DocsNavigation from '../../app/[lang]/docs-navigation'
import { getDocsDictionary } from '../docsDictionary.mjs'
import { DOCS_LOCALES, DOCS_PAGE_PATHS, buildDocsPageUrl } from '../docsSite.mjs'

const mockedNavigation = vi.hoisted(() => ({ pathname: '/' }))

vi.mock('next/navigation', () => ({
  usePathname: () => mockedNavigation.pathname
}))

function findCurrentPageLinks(lang, pathname) {
  mockedNavigation.pathname = pathname
  const markup = renderToStaticMarkup(
    createElement(DocsNavigation, { lang, navItems: getDocsDictionary(lang).nav })
  )
  return [...markup.matchAll(/<a href="([^"]+)" aria-current="page">/g)].map(([, href]) => href)
}

describe('문서 사이드바의 현재 페이지 표시', () => {
  const localizedPages = DOCS_LOCALES.flatMap((lang) =>
    DOCS_PAGE_PATHS.map((pagePath) => [lang, buildDocsPageUrl('', lang, pagePath)])
  )

  it.each(localizedPages)('%s 로케일의 %s에서는 그 메뉴 하나만 현재 페이지로 표시한다', (lang, pathname) => {
    expect(findCurrentPageLinks(lang, pathname)).toEqual([pathname])
  })

  it('메뉴에 없는 주소에서는 어떤 메뉴도 현재 페이지로 표시하지 않는다', () => {
    expect(findCurrentPageLinks('ko', '/ko/missing-page')).toEqual([])
  })
})
