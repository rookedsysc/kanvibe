/**
 * 문서 사이트 껍데기(내비게이션, 태그라인, 푸터)에 쓰는 로케일별 문구다.
 * 레이아웃과 구조화 데이터가 같은 문구를 써야 해서 한곳에 모아 둔다.
 */

const dictionaries = {
  ko: {
    tagline: 'AI 코딩 작업을 한눈에. 모바일에서도 이어서.',
    nav: [
      ['/', '홈'],
      ['/installation', '설치'],
      ['/quick-start', 'Quick Start'],
      ['/features', '기능'],
      ['/mobile', 'Mobile'],
      ['/settings', '설정'],
      ['/shortcuts', '단축키']
    ],
    repo: 'GitHub 저장소',
    poweredBy: 'Nextra 4 기반 문서'
  },
  en: {
    tagline: 'AI coding work on one board. Continue from your phone.',
    nav: [
      ['/', 'Home'],
      ['/installation', 'Installation'],
      ['/quick-start', 'Quick Start'],
      ['/features', 'Features'],
      ['/mobile', 'Mobile'],
      ['/settings', 'Settings'],
      ['/shortcuts', 'Shortcuts']
    ],
    repo: 'GitHub repository',
    poweredBy: 'Documentation powered by Nextra 4'
  },
  zh: {
    tagline: 'AI 编程任务一目了然，手机上继续工作。',
    nav: [
      ['/', '首页'],
      ['/installation', '安装'],
      ['/quick-start', 'Quick Start'],
      ['/features', '功能'],
      ['/mobile', '移动端'],
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
