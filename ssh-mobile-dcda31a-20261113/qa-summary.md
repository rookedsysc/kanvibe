# PR453 QA evidence

검증 커밋: `dcda31af9f8639d07d579643cd01df5e21ede797`
Git tree: `ab639c936645f44109633091c4146355eecdc4e5`
base: `origin/dev`20af2db. 작업 트리는 clean이며, 각 gate 직전 HEAD와 empty-diff fingerprint e69de29bb2d1d6434b8b29ae775ad8c2e48c5391을 기록했다. push한 커밋과 gate HEAD의 일치를 별도로 대조했다.

| 명령 | 종료/관측 |
|---|---|
| pnpm install --frozen-lockfile |0|
| pnpm check |0|
| pnpm test |0;204files/2895tests|
| pnpm build |0;renderer/main/standalone Bbundle|
| fvm flutter pub get |0|
| fvm dart format --output=none --set-exit-if-changed [변경7개 Dart파일] |0|
| fvm flutter analyze |0|
| fvm flutter test --concurrency=2 --reporter json |0;467/467|
| fvm flutter build apk --debug |0|
| git diff --check |0|
| 실제 격리 OpenSSH protocol |0;취소 뒤 master 생존/dead mux fallback 차단|
| 실제 격리 Herdr protocol + 최종B bundle integration |0;native입력/소유권거절/fullframe/명시resize→release복원|
| owner lease |0;열린stdin과14정상devicepongs에도Aheartbeat없으면약30초에1001/exit0|

Flutter 명령은 mobile/에서 FVM3.47.2/Dart3.13.2로 실행했다. Node24.15.0/pnpm10.17.0을 사용했다. 이전 검증 뒤 최신dev의 badge/DBsnapshot4커밋을 rebase했고 두 feature commit은 range-diff의 `=`로 내용 보존을 확인했다. 모바일 transport 단계의 신규 Node47개·Flutter17개와 앞선 SSH 회귀 사례가 모두 최종 전체 실행에 포함된다.

추가 변경파일 ESLint는 exit1: 기존 require import4오류+2경고. origin/dev 동일소스/rule/column과 대조해 신규진단0. CI의 필수 gate는 모두 위 명령대로 실행했다.

## Android journey
최종Maestro5 1/1PASS. DIRECT_OK/DIRECT_RESUMED 각1회, direct 중 A relay attach/input0. RELAY_OK1회, relay attach1/input9/controller1. unpair 뒤 pane이 사라지고 재시작해도 pairing이 제거된 상태를 확인했다. Root가 direct/relay 입력 PNG를 직접 열어 확인했다. 전용emulator5580에서 fixture5를 사용하고, 외부5556 기기는 건드리지 않았다.

## 검증 범위
Android는 실제TLS·최종 Bbundle·Electronroute와 메모리bridge/native-protocolactor를 사용했다. SSHmanagerbootstrap과 nativeHerdr는 별도의 격리 계약 검사다. 실제 사용자 B서버/회선 지연·설치앱 교체·iOS 실행은 검증하지 않았다. Xcode26.0.1이 요구26.2보다 낮아 iOS를 실행하지 않았다. B의 논리layout폭+viewport행 기준 복원을 검증했고 API미노출 실제PTY열수override는 미검증이다.

APK SHA256 `ae1d869055e145c71b8220f84dbab6915740f8a2f4ea62e35d90f65ada9448ab`
Bbundle SHA256 `0bd13b8ad6ed6985aefc07f1ed4bfbe56332a8f7be56fca775d1d4ede044fbf1`

GitHub PR453의 Type Check & Test와 Workers Builds: kanvibe 모두 SUCCESS를 확인했다.
