/**
 * 문서 사이트 껍데기(내비게이션, 태그라인, 푸터)에 쓰는 로케일별 문구다.
 * 레이아웃과 구조화 데이터가 같은 문구를 써야 해서 한곳에 모아 둔다.
 */

const dictionaries = {
  ko: {
    tagline: 'AI 코딩 작업을 한눈에. 휴대폰에서도 이어서.',
    structuredDataDescription: 'Claude Code, Codex, Gemini CLI, OpenCode 작업을 태스크별 git worktree와 터미널로 관리하는 macOS 칸반 앱입니다. iOS·Android 컴패니언 앱으로 휴대폰에서도 이어서 작업할 수 있습니다.',
    nav: [
      ['/', '홈'],
      ['/installation', '설치'],
      ['/quick-start', 'Quick Start'],
      ['/features', '기능'],
      ['/mobile', '모바일'],
      ['/settings', '설정'],
      ['/shortcuts', '단축키']
    ],
    repo: 'GitHub 저장소',
    poweredBy: 'Nextra 4 기반 문서'
  },
  en: {
    tagline: 'AI coding work on one board. Continue from your phone.',
    structuredDataDescription: 'macOS Kanban app that ties Claude Code, Codex, Gemini CLI, and OpenCode tasks to their own git worktrees and terminals, with an iOS/Android companion to continue from your phone.',
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
    structuredDataDescription: '为 Claude Code、Codex、Gemini CLI 和 OpenCode 任务分别连接 git worktree 与终端的 macOS 看板应用，并可通过 iOS/Android 配套应用在手机上继续工作。',
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
