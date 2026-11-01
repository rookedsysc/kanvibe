# 모바일 터미널 상단·탭 추가 QA

검증 커밋: `db1e0b2442dbd2fcb89db174a1b3c28fb8a39bcf`

Android 요청 동작 및 독립 구현 품질 PASS. 전체 harness UNVERIFIED: 기존 Dart format1파일과 iOS Xcode26.0.1/Firebase13 제약. 이번 푸시의 [Linux CI](https://github.com/rookedsysc/kanvibe/actions/runs/41287364699)와 Workers Builds는 성공했다. 로컬 macOS test의 단축키2실패는 변경 전 HEAD에서도 동일하게 재현했으며 Linux CI 성공과 구분한다.

- Flutter186/186, flutter analyze0, 변경 Dart10파일 format0, Android debug/release build0.
- Android16 emulator-5580, 실제 설치 APK SHA256이 빌드와 동일. native iOS 미검증.
- task-context-terminal1/1: 세로48×44·가로108×15, 상단 배지·동작 배치와 현재 브랜치에서 새 태스크 생성·shell 명령 확인.
- terminal-tabs1/1: 새 pane%294 자동 선택, 실제파일생성명령 실행. 기존 pane 유지, marker는 새 pane에만 있었다.
- terminal-clipboard1/1: OS Copy BEFOREYANK → 실제 Vim ggyy → OS Paste VIM_OSC52_NATIVE_OK.
- 모든 최종 여정에 Maestro 내부 startRecording MP4. 각 파일은 실행 종료 후540px/15fps로 압축. 실패했던 IME-off runner 여정은 제품 실패로 분류하지 않았으며 일반 LatinIME+녹화 wrapper에서 재실행해 통과했다.
- 24파일108hunk, 미매핑0·불필요한 동작 보존 변경0. 검증 시작 전 HEAD8b6ca11과 diff SHA25641e53a988aad8d7cf67ae396d609be0ba7063e0fd4d8ebe493cb517fc3f4fd27을 기록했고, push commit까지의 diff가 같다.
- 원격 SSH·Zellij·terminal 경계는 service tests; 실제 기기 여정은 local tmux이다. 동시에 다른 client가 새 탭을 만들 때 생성자별 정확한 ID 식별은 보장하지 않는다.

## 화면·영상·JUnit

- [terminal-portrait.png](https://github.com/rookedsysc/kanvibe/raw/qa-evidence/mobile-terminal-tabs-20261101-db1e0b24/mobile-terminal-tabs-20261101-db1e0b24/terminal-portrait.png)
- [terminal-landscape.png](https://github.com/rookedsysc/kanvibe/raw/qa-evidence/mobile-terminal-tabs-20261101-db1e0b24/mobile-terminal-tabs-20261101-db1e0b24/terminal-landscape.png)
- [new-terminal-tab.png](https://github.com/rookedsysc/kanvibe/raw/qa-evidence/mobile-terminal-tabs-20261101-db1e0b24/mobile-terminal-tabs-20261101-db1e0b24/new-terminal-tab.png)
- [native-vim-paste.png](https://github.com/rookedsysc/kanvibe/raw/qa-evidence/mobile-terminal-tabs-20261101-db1e0b24/mobile-terminal-tabs-20261101-db1e0b24/native-vim-paste.png)
- [context.mp4](https://github.com/rookedsysc/kanvibe/raw/qa-evidence/mobile-terminal-tabs-20261101-db1e0b24/mobile-terminal-tabs-20261101-db1e0b24/context.mp4)
- [context-junit.xml](https://github.com/rookedsysc/kanvibe/raw/qa-evidence/mobile-terminal-tabs-20261101-db1e0b24/mobile-terminal-tabs-20261101-db1e0b24/context-junit.xml)
- [tabs.mp4](https://github.com/rookedsysc/kanvibe/raw/qa-evidence/mobile-terminal-tabs-20261101-db1e0b24/mobile-terminal-tabs-20261101-db1e0b24/tabs.mp4)
- [tabs-junit.xml](https://github.com/rookedsysc/kanvibe/raw/qa-evidence/mobile-terminal-tabs-20261101-db1e0b24/mobile-terminal-tabs-20261101-db1e0b24/tabs-junit.xml)
- [clipboard.mp4](https://github.com/rookedsysc/kanvibe/raw/qa-evidence/mobile-terminal-tabs-20261101-db1e0b24/mobile-terminal-tabs-20261101-db1e0b24/clipboard.mp4)
- [clipboard-junit.xml](https://github.com/rookedsysc/kanvibe/raw/qa-evidence/mobile-terminal-tabs-20261101-db1e0b24/mobile-terminal-tabs-20261101-db1e0b24/clipboard-junit.xml)

이미지 branch/path는 인증된 GitHub API로 확인한다. 로그인 브라우저에서의 렌더링은 미확인이다.

## IME-off runner 실패 진단

- [clipboard-ime-off-failed.mp4](https://github.com/rookedsysc/kanvibe/raw/qa-evidence/mobile-terminal-tabs-20261101-db1e0b24/mobile-terminal-tabs-20261101-db1e0b24/clipboard-ime-off-failed.mp4)
- [clipboard-ime-off-home.png](https://github.com/rookedsysc/kanvibe/raw/qa-evidence/mobile-terminal-tabs-20261101-db1e0b24/mobile-terminal-tabs-20261101-db1e0b24/clipboard-ime-off-home.png)
- [clipboard-ime-off-junit.xml](https://github.com/rookedsysc/kanvibe/raw/qa-evidence/mobile-terminal-tabs-20261101-db1e0b24/mobile-terminal-tabs-20261101-db1e0b24/clipboard-ime-off-junit.xml)
- [before-yank-reset.png](https://github.com/rookedsysc/kanvibe/raw/qa-evidence/mobile-terminal-tabs-20261101-db1e0b24/mobile-terminal-tabs-20261101-db1e0b24/before-yank-reset.png)
