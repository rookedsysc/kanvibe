/**
 * 문서 사이트 껍데기(내비게이션, 태그라인, 푸터)에 쓰는 로케일별 문구다.
 * 레이아웃과 구조화 데이터가 같은 문구를 써야 해서 한곳에 모아 둔다.
 */

const dictionaries = {
  ko: {
    tagline: 'AI 에이전트를 위한 키보드 중심 칸반 워크스페이스',
    nav: [
      ['/', '홈'],
      ['/installation', '설치'],
      ['/quick-start', 'Quick Start'],
      ['/features', '기능'],
      ['/settings', '설정'],
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
      ['/quick-start', 'Quick Start'],
      ['/features', 'Features'],
      ['/settings', 'Settings'],
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
      ['/quick-start', 'Quick Start'],
      ['/features', '功能'],
      ['/settings', '设置'],
      ['/shortcuts', '快捷键']
    ],
    repo: 'GitHub 仓库',
    poweredBy: '文档由 Nextra 4 驱动'
  }
}

export function getDocsDictionary(locale) {
  return dictionaries[locale] ?? dictionaries.ko
}
