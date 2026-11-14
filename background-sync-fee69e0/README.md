# 백그라운드 sync / 정리 QA

검증 커밋: `fee69e05d610ac36cfe3f316143a5c645f18fd7c`. 게이트 전 HEAD85f3171 및 content fingerprint de2a2ee6b3ea90eb35eee995c47c8b90c77faf19와 커밋 변경 지문이 일치합니다.

- pnpm check/test/build 모두 exit0, 3479 tests.
- Flutter805/805, analyze/changed Dart format/APK build 모두 exit0.
- 실제 Electron7 checks PASS: count4→dialog→background/auto-close→정리중 재열기→Settings 왕복→부분 실패 retry→count2. 390px창 가로 넘침 없음.
- Android API36.1 arm64 / Maestro1 flow0failures exit0: sync 중복 시작 차단, 완료/재실행, count2→기존dialog→삭제 확인→auto-close→정리중 재열기→수동 닫기→실제 두 task/Git branch 삭제→count0. A/B 각1회.
- 실제 TypeORM 삭제 응답 원래 ID 반환 및 DB삭제 확인. 회귀 RED→GREEN.
- 독립 적합성/품질/아키텍처/범위 PASS, 블로커0. 검증용 프로세스/port/AVD 정리 완료.

MP4는 Maestro 내부 녹화(65.49초)입니다. 외부 Git/PR discovery만 fixture로 제어하며 IPC/HTTP/socket/SQLite/task resource/local Git삭제는 실제 실행했습니다. 설치된 앱과 DB, 공유 SDK/다른 장치는 변경하지 않았습니다.

iOS runtime은 기존 Xcode26.0.1/FirebaseSDK13.0.0 Swift tools6.2.1 불일치로 미검증입니다.
