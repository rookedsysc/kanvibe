'use client'

import { usePathname } from 'next/navigation'

function localizeHref(lang, href) {
  return href === '/' ? `/${lang}` : `/${lang}${href}`
}

export default function DocsNavigation({ lang, navItems }) {
  const pathname = usePathname()

  return (
    <nav>
      {navItems.map(([href, label]) => {
        const localizedHref = localizeHref(lang, href)

        return (
          <a
            key={href}
            href={localizedHref}
            aria-current={pathname === localizedHref ? 'page' : undefined}
          >
            {label}
          </a>
        )
      })}
    </nav>
  )
}
