# 모바일 기능 작업 워크트리의 PR 반영 범위

최초 기능46개·공유 API·후속 제목 겹침 수리·통합 작업을 포함해49개 워크트리를 만들었다. 그중26개는 기능별 작업 자리이며 실제 코드는13개 구현 묶음과 추가 검증·수리 작업에서 공유했다. 구현 중 공통 기준본을 복사했으므로 각 워크트리의 전체 Git diff는 독립 기능의 변경 범위가 아니다.

최신 `dev`를 기준으로 최종 소스를 통합하고 논리별 커밋으로 보존한다. 별도로 생성한 `fix/mobile-android-sdk-minor`는 Android 라이브러리의 SDK minor 설정 수리만 담는다. 통합 브랜치도 검증된 같은 빌드 수리 커밋을 포함한다. 두 PR 모두 base는 `dev`이며 빌드 PR의 변경이 통합 PR에도 포함되는 의존 관계를 명시한다.

`dev`에 이미 있는 알림 읽음 처리·채팅 첨부와 화면 배치·원격 relay는 정본을 유지하고, 여러 provider·역할/본문 검색·Mermaid·보드/태스크/터미널 관리와 설정을 결합했다. 별도 RouteObserver로 알림 읽음을 중복 실행하지 않는다. 분할 제목 겹침 수리는 최종 통합본의 후속 수리 커밋으로 보존한다.

| 원본 브랜치 | 작업 | 구현 묶음 | 처리 | 반영 브랜치 |
| --- | --- | --- | --- | --- |
| `feat/mobile-a01` | 실행 중 AI 세션 모니터링 | A01 | 공유 기준본 포함; 실제 변경분만 최종 통합 | `perf/not-implemented-mobile-feature` |
| `feat/mobile-a02` | Agent 호출 그래프·타임라인 | A01 | 공유 기준본 포함; 실제 변경분만 최종 통합 | `perf/not-implemented-mobile-feature` |
| `feat/mobile-a03` | 채팅 메시지 역할 필터 | A03 | 공유 기준본 포함; 실제 변경분만 최종 통합 | `perf/not-implemented-mobile-feature` |
| `feat/mobile-a04` | 여러 AI provider 동시 선택 | A04 | 공유 기준본 포함; 실제 변경분만 최종 통합 | `perf/not-implemented-mobile-feature` |
| `feat/mobile-a05` | 선택한 대화의 메시지 본문 검색 | A05 | 공유 기준본 포함; 실제 변경분만 최종 통합 | `perf/not-implemented-mobile-feature` |
| `feat/mobile-a06` | 채팅 내용 서식 렌더링 | A06 | 공유 기준본 포함; 실제 변경분만 최종 통합 | `perf/not-implemented-mobile-feature` |
| `feat/mobile-a07` | AI 계정 관리 | A01 | 공유 기준본 포함; 실제 변경분만 최종 통합 | `perf/not-implemented-mobile-feature` |
| `feat/mobile-a08` | AI 사용량 확인 | A01 | 빈 기능별 작업 자리 | `perf/not-implemented-mobile-feature` |
| `feat/mobile-b01` | 전체 태스크 빠른 검색 | B05 | 공유 기준본 포함; 실제 변경분만 최종 통합 | `perf/not-implemented-mobile-feature` |
| `feat/mobile-b02` | 현재 보드 안의 텍스트 찾기 | B05 | 빈 기능별 작업 자리 | `perf/not-implemented-mobile-feature` |
| `feat/mobile-b03` | 태스크 종류 필터 | B05 | 빈 기능별 작업 자리 | `perf/not-implemented-mobile-feature` |
| `feat/mobile-b04` | 여러 기준의 정렬과 설정 저장 | B05 | 빈 기능별 작업 자리 | `perf/not-implemented-mobile-feature` |
| `feat/mobile-b05` | Done 더보기와 전체 개수 | B05 | 공유 기준본 포함; 실제 변경분만 최종 통합 | `perf/not-implemented-mobile-feature` |
| `feat/mobile-b06` | 카드 우선순위와 상속 우선순위 | B05 | 빈 기능별 작업 자리 | `perf/not-implemented-mobile-feature` |
| `feat/mobile-b07` | 카드의 추가 요약 정보 | B05 | 빈 기능별 작업 자리 | `perf/not-implemented-mobile-feature` |
| `feat/mobile-b08` | 카드의 PR 바로 열기 | B05 | 빈 기능별 작업 자리 | `perf/not-implemented-mobile-feature` |
| `feat/mobile-b09` | Workspace 보드 | B05 | 공유 기준본 포함; 실제 변경분만 최종 통합 | `perf/not-implemented-mobile-feature` |
| `feat/mobile-g01` | 변경량 요약 | G02 | 빈 기능별 작업 자리 | `perf/not-implemented-mobile-feature` |
| `feat/mobile-g02` | Git Diff 화면 | G02 | 공유 기준본 포함; 실제 변경분만 최종 통합 | `perf/not-implemented-mobile-feature` |
| `feat/mobile-g03` | Diff 파일 편집·저장 | G02 | 빈 기능별 작업 자리 | `perf/not-implemented-mobile-feature` |
| `feat/mobile-g04` | 파일별 Viewed 체크 | G02 | 빈 기능별 작업 자리 | `perf/not-implemented-mobile-feature` |
| `feat/mobile-n01` | 태스크 상세 포커스 시 관련 알림 자동 읽음 | N01 | dev #458 정본 유지 + 보이는 태스크 알림 갱신 통합 | `perf/not-implemented-mobile-feature` |
| `feat/mobile-n02` | 운영 이벤트의 실시간 경고 | N02 | 공유 기준본 포함; 실제 변경분만 최종 통합 | `perf/not-implemented-mobile-feature` |
| `feat/mobile-p01` | SSH 프로젝트 탐색·등록 | S01 | 빈 기능별 작업 자리 | `perf/not-implemented-mobile-feature` |
| `feat/mobile-p02` | 등록 프로젝트 삭제 | S01 | 빈 기능별 작업 자리 | `perf/not-implemented-mobile-feature` |
| `feat/mobile-parity-api` | 공유 Desktop/API·Mermaid 제공 기반 | 공유/통합/수리 | 공유 기준본 포함; 실제 변경분만 최종 통합 | `perf/not-implemented-mobile-feature` |
| `feat/mobile-r01` | 탭 이름 변경 | G02 | 빈 기능별 작업 자리 | `perf/not-implemented-mobile-feature` |
| `feat/mobile-r02` | 탭 순서 변경 | G02 | 빈 기능별 작업 자리 | `perf/not-implemented-mobile-feature` |
| `feat/mobile-r03` | Pane 레이아웃 템플릿 편집 | G02 | 빈 기능별 작업 자리 | `perf/not-implemented-mobile-feature` |
| `feat/mobile-r04` | 분할 pane의 동시 표시 | G02 | 공유 기준본 포함; 실제 변경분만 최종 통합 | `perf/not-implemented-mobile-feature` |
| `feat/mobile-r05` | 원격 터미널 이미지 붙여넣기 | G02 | 빈 기능별 작업 자리 | `perf/not-implemented-mobile-feature` |
| `feat/mobile-s01` | 기본 세션 종류 변경 | S01 | 공유 기준본 포함; 실제 변경분만 최종 통합 | `perf/not-implemented-mobile-feature` |
| `feat/mobile-s02` | 기존 태스크의 세션 종류 일괄 변경 | S01 | 빈 기능별 작업 자리 | `perf/not-implemented-mobile-feature` |
| `feat/mobile-s03` | 시스템·라이트·다크 테마 선택 | S03 | 공유 기준본 포함; 실제 변경분만 최종 통합 | `perf/not-implemented-mobile-feature` |
| `feat/mobile-s04` | 다국어 화면 | S03 | 빈 기능별 작업 자리 | `perf/not-implemented-mobile-feature` |
| `feat/mobile-s05` | PC 백그라운드 동기화 제어 | S01 | 빈 기능별 작업 자리 | `perf/not-implemented-mobile-feature` |
| `feat/mobile-s06` | 동기화 결과 검토와 후속 처리 | S01 | 빈 기능별 작업 자리 | `perf/not-implemented-mobile-feature` |
| `feat/mobile-t01` | 생성 시 세션 종류 선택 | T02 | 빈 기능별 작업 자리 | `perf/not-implemented-mobile-feature` |
| `feat/mobile-t02` | 기존 태스크 설명 편집 | T02 | 공유 기준본 포함; 실제 변경분만 최종 통합 | `perf/not-implemented-mobile-feature` |
| `feat/mobile-t03` | 기존 태스크 우선순위 변경 | T02 | 빈 기능별 작업 자리 | `perf/not-implemented-mobile-feature` |
| `feat/mobile-t04` | 프로젝트 색상 변경 | T02 | 공유 기준본 포함; 실제 변경분만 최종 통합 | `perf/not-implemented-mobile-feature` |
| `feat/mobile-t05` | 관련 태스크 바로가기 | T02 | 빈 기능별 작업 자리 | `perf/not-implemented-mobile-feature` |
| `feat/mobile-t06` | 생성일·수정일 표시 | T06 | 공유 기준본 포함; 실제 변경분만 최종 통합 | `perf/not-implemented-mobile-feature` |
| `feat/mobile-t07` | 수동 Pending 이동 | T02 | 빈 기능별 작업 자리 | `perf/not-implemented-mobile-feature` |
| `feat/mobile-t08` | Done 이동 안내와 다시 묻지 않기 | T02 | 공유 기준본 포함; 실제 변경분만 최종 통합 | `perf/not-implemented-mobile-feature` |
| `feat/mobile-t09` | 저장 URL이 없을 때 PR 탐색 | T02 | 빈 기능별 작업 자리 | `perf/not-implemented-mobile-feature` |
| `feat/mobile-t10` | 기존 태스크를 브랜치 작업으로 전환 | T02 | 빈 기능별 작업 자리 | `perf/not-implemented-mobile-feature` |
| `fix/mobile-pane-title-overlap` | 분할 터미널 제목 영역 겹침 후속 수리 | 공유/통합/수리 | 공유 기준본 포함; 실제 변경분만 최종 통합 | `perf/not-implemented-mobile-feature` |
| `perf/not-implemented-mobile-feature` | 전체 기능 최종 통합·충돌 해소 | 공유/통합/수리 | 공유 기준본 포함; 실제 변경분만 최종 통합 | `perf/not-implemented-mobile-feature` |
| `fix/mobile-android-sdk-minor` | Android library compileSdk minor 설정 보존 | 독립 빌드 수리 | 게시 준비 중 요청한 CLI로 추가 생성 | `fix/mobile-android-sdk-minor` |

원본 워크트리·스냅샷·QA 실행 상태는 로컬 보관한다. 대용량 영상·스크린샷·도구 캐시는 기능 diff에 넣지 않고 PR QA 코멘트에서 검증한 커밋과 증거를 연결한다. 빈 작업 자리에 빈 커밋이나 중복 PR을 만들지는 않는다.
