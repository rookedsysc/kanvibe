# Mobile parity implementation

Desktop 기준 46개 기능의 소스를 구현하고 현재 작업 워크트리에 통합했다. API 의존성을 먼저 제공한 뒤 보드·AI·Git/터미널·설정을 병렬로 구현했다. 기능별 생성 워크트리와 실제 구현 코호트는 아래에 구분했다. 외부 OAuth, 실제 SSH, iOS 및 푸시 공급자 검증은 해당 환경 증거가 없으면 미검증으로 남긴다.

## 구현 추적

| ID | 기능 | 기능 워크트리 | 구현 코호트 |
| --- | --- | --- | --- |
| B01 | 전체 태스크 빠른 검색 | `feat-mobile-b01` | `B05` |
| B02 | 현재 보드 안의 텍스트 찾기 | `feat-mobile-b02` | `B05` |
| B03 | 태스크 종류 필터 | `feat-mobile-b03` | `B05` |
| B04 | 여러 기준의 정렬과 설정 저장 | `feat-mobile-b04` | `B05` |
| B05 | Done 더보기와 전체 개수 | `feat-mobile-b05` | `B05` |
| B06 | 카드 우선순위와 상속 우선순위 | `feat-mobile-b06` | `B05` |
| B07 | 카드의 추가 요약 정보 | `feat-mobile-b07` | `B05` |
| B08 | 카드의 PR 바로 열기 | `feat-mobile-b08` | `B05` |
| B09 | Workspace 보드 | `feat-mobile-b09` | `B05` |
| T01 | 생성 시 세션 종류 선택 | `feat-mobile-t01` | `T02` |
| T02 | 기존 태스크 설명 편집 | `feat-mobile-t02` | `T02` |
| T03 | 기존 태스크 우선순위 변경 | `feat-mobile-t03` | `T02` |
| T04 | 프로젝트 색상 변경 | `feat-mobile-t04` | `T02` |
| T05 | 관련 태스크 바로가기 | `feat-mobile-t05` | `T02` |
| T06 | 생성일·수정일 표시 | `feat-mobile-t06` | `T06` |
| T07 | 수동 Pending 이동 | `feat-mobile-t07` | `T02` |
| T08 | Done 이동 안내와 다시 묻지 않기 | `feat-mobile-t08` | `T02` |
| T09 | 저장 URL이 없을 때 PR 탐색 | `feat-mobile-t09` | `T02` |
| T10 | 기존 태스크를 브랜치 작업으로 전환 | `feat-mobile-t10` | `T02` |
| G01 | 변경량 요약 | `feat-mobile-g01` | `G02` |
| G02 | Git Diff 화면 | `feat-mobile-g02` | `G02` |
| G03 | Diff 파일 편집·저장 | `feat-mobile-g03` | `G02` |
| G04 | 파일별 Viewed 체크 | `feat-mobile-g04` | `G02` |
| A01 | 실행 중 AI 세션 모니터링 | `feat-mobile-a01` | `A01` |
| A02 | Agent 호출 그래프·타임라인 | `feat-mobile-a02` | `A01` |
| A03 | 채팅 메시지 역할 필터 | `feat-mobile-a03` | `A03` |
| A04 | 여러 AI provider 동시 선택 | `feat-mobile-a04` | `A04` |
| A05 | 선택한 대화의 메시지 본문 검색 | `feat-mobile-a05` | `A05` |
| A06 | 채팅 내용 서식 렌더링 | `feat-mobile-a06` | `A06` |
| A07 | AI 계정 관리 | `feat-mobile-a07` | `A01` |
| A08 | AI 사용량 확인 | `feat-mobile-a08` | `A01` |
| R01 | 탭 이름 변경 | `feat-mobile-r01` | `G02` |
| R02 | 탭 순서 변경 | `feat-mobile-r02` | `G02` |
| R03 | Pane 레이아웃 템플릿 편집 | `feat-mobile-r03` | `G02` |
| R04 | 분할 pane의 동시 표시 | `feat-mobile-r04` | `G02` |
| R05 | 원격 터미널 이미지 붙여넣기 | `feat-mobile-r05` | `G02` |
| P01 | SSH 프로젝트 탐색·등록 | `feat-mobile-p01` | `S01` |
| P02 | 등록 프로젝트 삭제 | `feat-mobile-p02` | `S01` |
| S01 | 기본 세션 종류 변경 | `feat-mobile-s01` | `S01` |
| S02 | 기존 태스크의 세션 종류 일괄 변경 | `feat-mobile-s02` | `S01` |
| S03 | 시스템·라이트·다크 테마 선택 | `feat-mobile-s03` | `S03` |
| S04 | 다국어 화면 | `feat-mobile-s04` | `S03` |
| S05 | PC 백그라운드 동기화 제어 | `feat-mobile-s05` | `S01` |
| S06 | 동기화 결과 검토와 후속 처리 | `feat-mobile-s06` | `S01` |
| N01 | 태스크 상세 포커스 시 관련 알림 자동 읽음 | `feat-mobile-n01` | `N01` |
| N02 | 운영 이벤트의 실시간 경고 | `feat-mobile-n02` | `N02` |

## 검증

- Desktop: 최종 `pnpm check`, `pnpm test` 2,955/2,955, `pnpm build` 통과. Android 기기 검증은 아래 실행 기록에서 확정한다. 이전 수리 단계의 기존 Workspace 사례 1건 실패는 단독 및 전체 재실행에서 통과했고, 이번 최종 전체 실행에는 실패가 없었다.
- Flutter: 최종 통합 회귀 627/627 통과, skipped 0. `flutter analyze`, 전체 `dart format --output=none --set-exit-if-changed lib test` 통과. 실제 Android 검증은 아래 실행 기록에서 구분한다.
- Android: 검색 수리를 포함한 candidate09의 검증용 x86_64 debug/release APK 빌드 통과. 두 빌드의 입력 SHA는 전후 일치하며 기존 기기 설치본과 서명이 같다. 실제 기기 화면 판정은 QA 기록에서 별도로 확인한다.
- 고친 회귀: Done 안내 저장 시 provider 수명, PC 전환 중 늦은 응답, AI focus의 이중 pop, 색상창 퇴장 시 controller dispose, 채팅 검색 초안 지우기, 본문 검색 결과가 없을 때 404가 반환되는 문제, 검색 결과의 프로젝트 색상 누락, Workspace 생성 버튼의 터미널 겹침, tmux 분할 시 작업 폴더 이탈.
- 설치한 디자인 지침: `taste-skill`, `ui-ux-pro-max`. 기존 Flutter 토큰·48dp 터치 영역·반응형 레이아웃·오류/재시도·라이트 테마 대비에 적용했다.

## 산출물

- 원본 조사: [mobile-desktop-feature-parity.md](mobile-desktop-feature-parity.md)
- 상세 실행·실패·수리 로그: 작업 디렉터리 `qa-results/roky-harness/mobile-parity-20261113/`
- 제품 코드의 커밋·푸시·PR은 수행하지 않았다. 격리 QA Git 저장소의 초기 fixture commit은 별개다.


Android 첫 빌드에서 `flutter_secure_storage` library의 SDK minor 설정이 app에 전파되지 않는 문제가 드러났다. API37을 쓰는 library에 `compileSdkMinor=0`을 DSL finalization으로 적용해 APK를 빌드했다. SDK minor 속성은 [Android Gradle 공식 API](https://developer.android.com/reference/tools/gradle-api/9.0/com/android/build/api/dsl/LibraryExtension)에 따른다. SDK 폴더 위장이나 의존성 하향은 하지 않았다.

실제 Android QA는 라이트 터미널 chrome의 대비와 Mermaid WebView 실패를 발견했다. 고정 dark chrome의 foreground를 명시하고 WCAG 대비 회귀를 추가했다. 소유 AVD의 native WebView는 문서를 표시하지 못하고 renderer SIGTRAP으로 종료돼, Mermaid를 연결된 Desktop에서 로컬 이미지로 렌더링하는 경로로 바꿨다. 모바일의 WebView 의존성과 번들은 제거했다.

Desktop renderer는 외부 네트워크를 차단한 전용 offscreen 창을 사용한다. 사용자 창 조회·알림·보드 이벤트·활성화 경로는 renderer를 제외한다. 요청 크기·이미지 크기·대기열·캐시·시간 제한을 적용하며, 모바일은 PC와 토큰이 달라진 응답을 버린다. 실제 이미지 디코딩이 끝난 후에만 준비 완료 상태를 표시한다.

검증용 Android에서는 정상 PNG도 native frame decode가 실패했다. 실제 기기 메모리에서 추출한 PNG의 CRC·압축·픽셀 데이터와 호스트 Flutter 디코딩은 정상이었으며, 일반 배열 복사·다른 PNG 인코딩·엔진 전환으로도 해결되지 않았다. 같은 그래프의 JPEG는 실제 기기에서 읽혔으므로, 불투명 배경을 유지하는 최고 품질 JPEG를 제공하고 MIME을 명시하도록 수리했다. 모바일은 JPEG와 기존 PNG 응답을 모두 검증한다. 전역 렌더러 설정이나 새 플러그인은 추가하지 않았다.

실제 Chromium JPEG 네 종류를 Android 기본 Impeller에서 일반·읽기 전용 배열로 읽는 8개 대조가 통과했다. 별도로 새 검증용 Desktop에 QA 기기를 페어링해 인증서 고정 TLS → 실제 JPEG 응답 → production repository → Flutter codec의 light/dark 응답을 확인했다. QA 기기는 검증 후 해제하고 401 응답을 확인했으며, 기존 Android 페어링은 보존했다. 최종 APK의 화면·확대·원문·복사 판정은 기기 QA 보고서에서 구분한다.

운영 경고 릴레이는 별도 `node --test services/mobile-push-relay/relay.node-test.mjs` 20/20 통과했다. 새 운영 경고 전달 테스트는 변경 전 서버의 400 응답에서 실패하고 현재 서버의 200 응답에서 통과함을 확인했다. 외부 FCM 공급자에는 실제로 발송하지 않았다.


최종 소스의 로컬 명령은 `pnpm check`, `pnpm test`, `pnpm build`, `flutter analyze`, `dart format --output=none --set-exit-if-changed lib test`, `flutter test --machine`, Android debug/release x86_64 빌드가 모두 exit 0이다. 각 명령의 HEAD·입력 fingerprint·전후 일치 여부와 원본 출력은 QA 실행 폴더에 보존했다. 릴리스 APK는 기기 검증용 debug 서명이며 배포용 서명이나 ARM64·iOS 실행을 대신하지 않는다.

외부 CI의 `Workers Builds: kanvibe`는 Cloudflare 대시보드 작업으로, 로컬 실행이나 배포를 수행하지 않았다. SDK 빌드 수리인 `mobile/android/build.gradle.kts`의 compileSdkMinor finalization hunk는 기능 구현과 구분해 별도 커밋 대상으로 기록했다. 제품 커밋은 아직 만들지 않았다.


채팅 검색 수리는 격리 Desktop의 실제 인증서 고정 TLS 요청으로도 검증했다. 존재하는 대화의 전체 메시지 2개·일치 검색 1개·불일치 검색 0개를 확인했고, 불일치는 200을 반환하며 존재하지 않는 대화는 404를 유지했다. 검증용 새 기기만 페어링·해제했다. 원본 기록은 `final-search-paired-http-contract.json` 및 `device/contracts/chat-query-fixture-verification.json`이다.

## 최종 기기 검증 범위

Android42개 기능에 제한된 실제 시나리오 증거가 있으며,4개는 환경 의존 경로가 미검증이다. 이는46개 전체 E2E 통과를 의미하지 않는다. PR 성공 경로, 비어 있지 않은 실제 Agent 그래프, SSH·이미지 전송, OAuth 변경, 외부 푸시, iOS·ARM64 실행 검증은 별도로 남는다. 색상·Workspace 배치·분할 폴더 회귀는 새 candidate09에서 다시 통과했다. 상세 범위와 실패·복구·녹화 증거는 `qa-results/roky-harness/mobile-parity-20261113/device/final-delivery-qa.md` 및 `feature-device-evidence.json`에 기록했다.

기능46개 워크트리는 요청한 CLI로 생성했다. 공용 구현은 표의 코호트에서 병렬 작업했으며, 후속 색상·Workspace·분할 폴더 수리는 각각 T04·B09·R04 개별 워크트리에서 수행했다. 구현 소스와 테스트는 현재 작업 브랜치에 통합되어 있고 제품 커밋·푸시·PR은 만들지 않았다.

## QA 재실행 · 2026-11-13

Desktop2,955·Flutter627·Relay20 테스트와 정적 검사·빌드가 다시 통과했다. 보드/환경설정·채팅·Git/정보·Workspace/분할·PC 설정의 주요 Android 시나리오도 재실행했다. 실제 PR433의 null 캐시 조회·캐시 저장·Android VIEW URL 전달은 추가 확인했다. 캐시 카드 재열기는 자동화 복귀/기기 발견 실패로 미검증이며 스텁 브라우저의 실제 페이지 로딩도 검증하지 않았다.

**열린 UI 결함1건:** 가로 분할 터미널에서 prompt 출력이 pane 제목 띠에 겹치는 QA-RTRY-01(P2)을 재진입 화면에서도 확인했다. 제품 코드는 이번 QA에서 수정하지 않았다. 소유 QA 자원 정리·README 복원·Node24 SQLite 복원과 최종 source fingerprint 불변을 확인했다.46개 전체 E2E 통과 판정은 아니다. [재실행 보고서](../../qa-results/roky-harness/mobile-parity-retry-20261113-0956/report.md), [상세 요약](../../qa-results/roky-harness/mobile-parity-retry-20261113-0956/summary.json).

## QA-RTRY-01 수정 완료 · 2026-11-13

분할 터미널의 내용 영역에 명시적 ClipRect를 추가해 출력이 pane 제목을 침범하지 않게 수정했다. 수정 전 실패/후 통과한 실제 paint 회귀를 포함해 Flutter628개 테스트·포맷·분석·새 Android release 빌드가 통과했다. 실제 Android의 분할·80줄 출력 스크롤·정보창 복귀·태스크 재진입 화면에서도 두 제목이 깨끗함을 확인했다. QA-RTRY-01은 해결 상태다. 요청한 CLI로 별도 수정 워크트리를 만들었고 현재 통합 작업 디렉터리에도 두 파일만 반영했다. [수정·검증 보고서](../../qa-results/roky-harness/pane-title-fix-20261113/report.md).

## dev 기준 PR 통합 준비

게시 요청에 따라 최신 dev의 채팅·첨부·원격 relay·알림 읽음 처리를 보존해 통합했다. 알림 읽음의 RouteObserver 중복 경로는 제거하고 기존 inbox controller를 재사용한다. 실제 보이는 태스크에 새 알림이 도착했을 때도 같은 경로로 갱신한다. MultiPaneTerminal에는 최신 dev의 활성 상태·채팅 전환 callback을 전달하며 선택 변경 시 기존 pane 구독과 스크롤 상태를 보존한다. 원본49개 워크트리와 별도 SDK 빌드 작업의 반영 범위는 [워크트리 대응표](mobile-parity-pr-worktree-map.md)에 기록했다. 커밋별 정확한 검사 결과와 시각 증거는 PR QA 코멘트에 연결한다.
