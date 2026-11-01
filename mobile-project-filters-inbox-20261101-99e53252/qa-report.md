# 모바일 프로젝트 필터·데스크톱 알림함 QA

검증 기준: HEAD `db1e0b2442dbd2fcb89db174a1b3c28fb8a39bcf` + 미커밋 변경 SHA-256 `99e5325287e1dcaf44735577f756ca5b06c739234319035b35bffc35d1195bde`.

Android 기능 적합성 PASS, 독립 구현 품질 PASS. 전체 harness UNVERIFIED: 기존 테스트·포맷 실패와 iOS/원격 CI 제약을 분리했다. 기능 소스는 작업트리 기준이다.

- 프로젝트 검색·복수 선택·전체 해제, 같은 호스트의 worktree 포함, PC별 필터 유지.
- 현재 PC 알림함·미읽음 배지·전체/안 읽음·개별/전체 읽음·최신 태스크 이동.
- 기존 desktop notificationStore의 생성/읽음을 직렬화하고 desktop IPC와 인증된 모바일 events로 변경 통지.
- API: GET /api/mobile/notifications, bodyless PATCH /api/mobile/notifications/:id/read, bodyless POST /api/mobile/notifications/read-all. 401/404/쓰기 실패·동시성 검증.

| 최종 명령 | exit / 결과 |
| --- | --- |
| pnpm install --frozen-lockfile | 0 |
| pnpm check | 0 |
| pnpm test | 1, 1813/1815, 아래 기존 실패2 |
| pnpm build | 0 |
| fvm flutter analyze | 0 |
| fvm flutter test --reporter=json | 0,234/234(신규48/관련91) |
| 변경 Dart19개 format --output=none --set-exit-if-changed | 0 |
| fvm dart format --output=none --set-exit-if-changed . | 1, 기존/생성 파일13 |
| fvm flutter build apk --debug | 0 |
| fvm flutter build apk --release | 0 |
| fvm flutter build ios --debug --simulator --no-codesign | 1, 아래 툴체인 제약 |
| APP_ID=com.kanvibe.kanvibe_mobile MAESTRO_PLATFORM=android bash scripts/maestro/run-maestro-e2e.sh project-filter-notifications | 0,1/1,필수녹화 |

모든 최종 게이트의 시작/종료 HEAD와 내용 지문이 위 작업트리와 일치했다. Node24.15.0/pnpm10.17.0/macOS arm64, Flutter3.47.2/Dart3.13.2, Maestro2.10.0, Android16/API36 emulator-5590.

실제 Electron fixture를 Page.reload한 뒤 Android 흐름을 실행했다. 저장 알림4/미읽음3 →5/0, 실제 renderer IPC 변경 통지0→5. 개별 읽음(모바일), 읽음(데스크톱), 새 알림, 최신 태스크와 전체 읽음을 확인했다. 대상 task는 터미널 없는 fixture이고 최신 제목은 Maestro Open Task Latest다. 화면 헤더는 생략되므로 접근성 문구/서버 task 값으로 최신값을 확인했다.

읽음 대기·새로고침·새 알림의 경합은 구독 상태 테스트에서 RED(expected during-refresh, actual[n1]) 후 한 조건 수정으로 GREEN. 안 읽음 필터 뒤 행 child semantics가 이전 ID를 유지한 native 실패는 row key를 알림 ID로 고정한 뒤 동일 엄격한 선택자로 해결했다. 첫 카드/후속 헤더 테스트는 실제 합쳐진 접근성 문구에 맞춰 multiline selector로 고쳤다. 기존 brand token 선택색도 적용했다.

| 전수 검토 파일 | hunks | 미매핑 | 무관한 동작 보존 | gate-enabling-repair | tooling-enforced-format |
| --- | --- | --- | --- | --- | --- |
| 31 | 82 | 0 | 0 | 0 | 0 |

기존 실패/제약:
- macOS appSettings 단축키 승계 테스트2개: exact HEAD archive와 후보가 같은16/18·같은 실패 이름2개. 신규 backend26개는 모두 통과.
- full format13파일은 HEAD와 같은 create_task_screen.dart와 ignored Firebase 생성/예제12파일. 변경19파일은 통과.
- Xcode26.0.1은 Firebase13의 Swift tools6.2.1 의존성을 해석하지 못해 iOS build/device 검증 불가.
- 새 미커밋 소스의 Linux Actions/Workers 원격 체크는 미실행. 기존 PR db1e0b2의 성공 체크는 이번 결과로 재사용하지 않았다. Docs workflow는 이번 변경 경로에 적용되지 않는다.
- 실제 FCM/APNs나 물리 기기 전달은 이번 알림함 검증에 포함하지 않았다.

증거: [성공 영상](https://github.com/rookedsysc/kanvibe/raw/qa-evidence/mobile-project-filters-inbox-20261101-99e53252/mobile-project-filters-inbox-20261101-99e53252/journey.mp4), [JUnit](https://github.com/rookedsysc/kanvibe/raw/qa-evidence/mobile-project-filters-inbox-20261101-99e53252/mobile-project-filters-inbox-20261101-99e53252/junit.xml), [행 ID 수리 전 영상](https://github.com/rookedsysc/kanvibe/raw/qa-evidence/mobile-project-filters-inbox-20261101-99e53252/mobile-project-filters-inbox-20261101-99e53252/row-identity-before.mp4), [수리 전 hierarchy](https://github.com/rookedsysc/kanvibe/raw/qa-evidence/mobile-project-filters-inbox-20261101-99e53252/mobile-project-filters-inbox-20261101-99e53252/row-identity-before.json), [테스트 계수](https://github.com/rookedsysc/kanvibe/raw/qa-evidence/mobile-project-filters-inbox-20261101-99e53252/mobile-project-filters-inbox-20261101-99e53252/test-counts.json).

스크린샷: [프로젝트/worktree](https://github.com/rookedsysc/kanvibe/raw/qa-evidence/mobile-project-filters-inbox-20261101-99e53252/mobile-project-filters-inbox-20261101-99e53252/project-filter-alpha-worktree.png), [복수 선택](https://github.com/rookedsysc/kanvibe/raw/qa-evidence/mobile-project-filters-inbox-20261101-99e53252/mobile-project-filters-inbox-20261101-99e53252/project-filter-multiple-projects.png), [안 읽음](https://github.com/rookedsysc/kanvibe/raw/qa-evidence/mobile-project-filters-inbox-20261101-99e53252/mobile-project-filters-inbox-20261101-99e53252/notifications-unread.png), [데스크톱 실시간](https://github.com/rookedsysc/kanvibe/raw/qa-evidence/mobile-project-filters-inbox-20261101-99e53252/mobile-project-filters-inbox-20261101-99e53252/notifications-desktop-live-sync.png), [최신 task](https://github.com/rookedsysc/kanvibe/raw/qa-evidence/mobile-project-filters-inbox-20261101-99e53252/mobile-project-filters-inbox-20261101-99e53252/notification-opens-latest-task.png), [모두 읽음](https://github.com/rookedsysc/kanvibe/raw/qa-evidence/mobile-project-filters-inbox-20261101-99e53252/mobile-project-filters-inbox-20261101-99e53252/notifications-all-read.png).

GitHub 인증 API로 업로드 경로/blob을 검증한다. 로그인 브라우저에서 이미지 렌더링 여부는 별도로 확인하지 않았다.
