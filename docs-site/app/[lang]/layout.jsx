import LanguageSwitcher from './language-switcher'
import { getDocsDictionary } from '../../lib/docsDictionary.mjs'
import { resolveDocsSiteUrl } from '../../lib/docsSite.mjs'
import './styles.css'

const siteUrl = resolveDocsSiteUrl()

export const metadata = {
  metadataBase: new URL(siteUrl),
  title: {
    default: 'KanVibe Docs',
    template: '%s · KanVibe Docs'
  },
  description: 'KanVibe documentation for installation, quick start, features, settings, and keyboard shortcuts.',
  applicationName: 'KanVibe Docs',
  generator: 'Nextra',
  openGraph: {
    title: 'KanVibe Docs',
    description: 'Keyboard-first Kanban workspace for AI coding agents.',
    siteName: 'KanVibe Docs',
    type: 'website'
  }
}

function localizeHref(lang, href) {
  return href === '/' ? `/${lang}` : `/${lang}${href}`
}

function KanVibeLogo({ lang, tagline }) {
  return (
    <a className="kv-logo" href={`/${lang}`} aria-label="KanVibe Docs">
      <span className="kv-logo-mark">K</span>
      <span className="kv-logo-copy">
        <strong>KanVibe</strong>
        <small>{tagline}</small>
      </span>
    </a>
  )
}

export default async function RootLayout({ children, params }) {
  const { lang } = await params
  const dictionary = getDocsDictionary(lang)

  return (
    <html lang={lang} dir="ltr" suppressHydrationWarning>
      <body>
        <div className="kv-doc-shell">
          <header className="kv-topbar">
            <KanVibeLogo lang={lang} tagline={dictionary.tagline} />
            <LanguageSwitcher currentLang={lang} />
          </header>
          <div className="kv-doc-layout">
            <aside className="kv-sidebar" aria-label="Documentation navigation">
              <nav>
                {dictionary.nav.map(([href, label]) => (
                  <a key={href} href={localizeHref(lang, href)}>{label}</a>
                ))}
              </nav>
              <a className="kv-repo-link" href="https://github.com/rookedsysc/kanvibe">
                {dictionary.repo} ↗
              </a>
            </aside>
            <main className="kv-content">
              {children}
            </main>
          </div>
          <footer className="kv-footer">
            <span>© {new Date().getFullYear()} KanVibe.</span>
            <span>{dictionary.poweredBy}</span>
          </footer>
        </div>
      </body>
    </html>
  )
}
