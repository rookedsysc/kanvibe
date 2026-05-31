import LanguageSwitcher from './language-switcher'
import './styles.css'

export const metadata = {
  metadataBase: new URL('https://github.com/rookedsysc/kanvibe'),
  title: {
    default: 'KanVibe Docs',
    template: '%s · KanVibe Docs'
  },
  description: 'KanVibe documentation for installation, features, usage, and keyboard shortcuts.',
  applicationName: 'KanVibe Docs',
  generator: 'Nextra',
  openGraph: {
    title: 'KanVibe Docs',
    description: 'Keyboard-first Kanban workspace for AI coding agents.',
    siteName: 'KanVibe Docs',
    type: 'website'
  }
}

const dictionaries = {
  ko: {
    tagline: 'AI 에이전트를 위한 키보드 중심 칸반 워크스페이스',
    nav: [
      ['/', '홈'],
      ['/installation', '설치'],
      ['/features', '기능'],
      ['/usage', '기본 사용법'],
      ['/shortcuts', '단축키']
    ],
    repo: 'GitHub 저장소',
    poweredBy: 'Nextra 4 기반 문서'
  },
  en: {
    tagline: 'Keyboard-first Kanban workspace for AI agents',
    nav: [
      ['/', 'Home'],
      ['/installation', 'Installation'],
      ['/features', 'Features'],
      ['/usage', 'Basic usage'],
      ['/shortcuts', 'Shortcuts']
    ],
    repo: 'GitHub repository',
    poweredBy: 'Documentation powered by Nextra 4'
  },
  zh: {
    tagline: '面向 AI 代理的键盘优先看板工作区',
    nav: [
      ['/', '首页'],
      ['/installation', '安装'],
      ['/features', '功能'],
      ['/usage', '基本用法'],
      ['/shortcuts', '快捷键']
    ],
    repo: 'GitHub 仓库',
    poweredBy: '文档由 Nextra 4 驱动'
  }
}

function getDictionary(lang) {
  return dictionaries[lang] ?? dictionaries.ko
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
  const dictionary = getDictionary(lang)

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
