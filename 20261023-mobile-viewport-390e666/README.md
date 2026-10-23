검증 대상 커밋: `390e666895212e900c8385c42301720567a84443`

<!-- kanvibe-mobile-viewport-390e666 -->

모바일 화면 맞춤 수정의 코드·계약 검토는 PASS이고, 필수 기기 여정은 **환경 문제로 미완료(UNVERIFIED)**입니다.

- Flutter **96/96**, 관련 backend/wire/parser **142/142** 통과. 신규 테스트는 Flutter5 + backend/wire7 = 12개입니다.
- `pnpm test`: **1750 통과 / 2 실패**. 수정 전 HEAD `4bb5a01` archive의 appSettingsService 단독 실행에서도 **16 통과 / 같은 2 실패**를 확인했습니다. 실패는 macOS 단축키 표기 승계 두 건이며 이 파일은 수정하지 않았습니다.
- 실제 격리 tmux의 source PTY에서 **44열×70행**을 확인했고 focus·layout·window-size 정책·zoom을 복원했습니다.
- 독립 source QA: 17개 파일/69개 hunk, 미매핑0, 잔여 blocker0. 기기 검증을 이 PASS에 포함하지 않습니다.

| 실행 명령 | 실행 cwd | exit |
|---|---|---:|
| `pnpm install --frozen-lockfile` | `.` | 0 |
| `pnpm check` | `.` | 0 |
| `pnpm test` | `.` | 1 |
| `pnpm build` | `.` | 0 |
| `pnpm exec vitest run src/desktop/main/services/__tests__/mobileBridgeService.test.ts electron/mobileRoutes.test.mjs src/lib/__tests__/paneMirror.test.ts --reporter=json --outputFile=qa-results/roky-harness/20261023-mobile-viewport/backend-report.json` | `.` | 0 |
| `fvm flutter test --reporter json` | `mobile` | 0 |
| `fvm dart format --output=none --set-exit-if-changed lib test` | `mobile` | 0 |
| `fvm flutter analyze` | `mobile` | 0 |
| `fvm flutter build apk --debug` | `mobile` | 0 |
| `fvm flutter build apk --release` | `mobile` | 0 |
| `pnpm exec tsx qa-results/roky-harness/20261023-mobile-viewport/tmux-proof.cjs` | `.` | 0 |
| `pnpm exec vitest run --config vitest.baseline.config.ts --reporter=json --outputFile=../../../baseline-shortcuts-final-report.json` | `qa-results/roky-harness/20261023-mobile-viewport/baseline/node_modules/kanvibe-baseline` | 1 |
| `bash scripts/maestro/run-maestro-e2e.sh viewport-portrait` | `.` | 137 |

모든 결과는 게이트 시작 전 HEAD `4bb5a0192aed6c62d26328213f1e1e3966bbaa4b`와 미커밋 변경 SHA-256 `ddb3fc06958c49d4c1e61e7e5e6d2756687cf7003c05fbc47d08764df5a5eac0`에 묶여 있습니다. push한 `390e666895212e900c8385c42301720567a84443`와의 diff를 다시 계산해 **같은 내용**임을 확인했습니다. 커밋 때 소스를 바꾸는 hook은 없었습니다.

Maestro는 `APP_ID=com.kanvibe.kanvibe_mobile`, Android, `MAESTRO_DRIVER_STARTUP_TIMEOUT=60000`, viewport-portrait flow와 정본 내부 startRecording wrapper로 실행했습니다. Galaxy USB 분리 후 API36.1 AVD로 대체했고, 재부팅·드라이버 재시작·task 소유 임시 userdata/cache·Swiftshader cold boot 후에도 System UI/Messages ANR와 gRPC `viewHierarchy` 정지가 관측됐습니다. 마지막 실행은 진행 없는 프로세스를 종료해 exit137입니다. **기기 terminal 화면·회전·IME·PC 복원 미검증, 완료 JUnit과 성공 여정 MP4 없음**입니다. 이 실패를 제품 회귀로 분류하지 않았습니다. 페어링 코드 화면이 포함된 일부 실패 캡처/영상은 공개 첨부에서 제외했고, Maestro 실패 recording은 moov atom이 없어 재생할 수 없습니다. 외부 Workers Builds는 로컬 tree에서 재현하지 않았습니다.

[기계 결과와 검토 자료](https://github.com/rookedsysc/kanvibe/blob/qa-evidence/20261023-mobile-viewport-390e666/20261023-mobile-viewport-390e666) · [테스트 수](https://github.com/rookedsysc/kanvibe/blob/qa-evidence/20261023-mobile-viewport-390e666/20261023-mobile-viewport-390e666/test-counts.json) · [실제 tmux 증거](https://github.com/rookedsysc/kanvibe/blob/qa-evidence/20261023-mobile-viewport-390e666/20261023-mobile-viewport-390e666/tmux-proof-results.json) · [독립 검토](https://github.com/rookedsysc/kanvibe/blob/qa-evidence/20261023-mobile-viewport-390e666/20261023-mobile-viewport-390e666/independent-review.md)

기기 환경 실패 화면 (제품 성공 화면이 아님):

![에뮬레이터 System UI ANR](https://github.com/rookedsysc/kanvibe/raw/qa-evidence/20261023-mobile-viewport-390e666/20261023-mobile-viewport-390e666/android-startup.png)

[새 임시 AVD에서 실행된 앱 연결 화면](https://github.com/rookedsysc/kanvibe/blob/qa-evidence/20261023-mobile-viewport-390e666/20261023-mobile-viewport-390e666/android-fresh.png)

자산 Git 경로와 인증된 contents API 응답은 게시 후 확인합니다. 로그인 브라우저에서의 이미지 렌더링은 별도로 확인하지 못했습니다.
