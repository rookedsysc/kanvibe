'use client'

import { usePathname } from 'next/navigation'

const languages = [
  ['ko', '한국어'],
  ['en', 'English'],
  ['zh', '中文']
]

function getLocalizedPath(pathname, locale) {
  const parts = pathname.split('/').filter(Boolean)
  if (parts.length === 0) {
    return `/${locale}`
  }

  const [, ...rest] = parts
  return `/${[locale, ...rest].join('/')}`
}

export default function LanguageSwitcher({ currentLang }) {
  const pathname = usePathname()

  return (
    <nav className="kv-locale-switch" aria-label="Language">
      {languages.map(([locale, label]) => (
        <a
          key={locale}
          href={getLocalizedPath(pathname, locale)}
          aria-current={locale === currentLang ? 'page' : undefined}
        >
          {label}
        </a>
      ))}
    </nav>
  )
}
