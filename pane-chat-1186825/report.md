검증 대상 커밋: 1186825fb543f1b6eba85aaf0510ed3d90458895

# 활성 pane CLI 채팅 QA

최종 기능 검증 PASS. 기존 feat/chat-ui 브랜치와 PR #443 유지.

- Gate HEAD: e22764a71755f7b6ce89d13882fc5a8c7e42060e
- Gate fingerprint: 20f4c8908cf45b3e3f7fef76e823929d3d36d538
- Node 24.15.0 / Flutter 3.47.2. 참조 clone·QA 복사본을 제외한 동일 tracked source snapshot에서 데스크톱 CI 실행. live-final-source.json manifest 대조.
- pnpm check: exit0
- pnpm test: exit0, 209 files / 2855 tests
- pnpm build: exit0
- flutter analyze --no-pub: exit0, no issues
- flutter test --no-pub --reporter json: exit0, 454 tests
- ESLint scoped: exit0
- git diff --check: exit0
- Electron live-fixture9: actual PTY·IPC·process fence·2pane격리·멀티라인·정확한JSONL·native모델선택·모델변경후응답 PASS. 화면 PNG 직접 확인.
- Herdr production service: isolated task/DB/native CLI 5/5 PASS exit0, own session stop/delete. live-backend/kanvibe-herdr-production-proof.json
- Android profile APK + Maestro live-maestro-verified: 1/1 flow PASS, recordingStatus recorded. 전송→응답→모델선택→변경후응답→terminal/chat복귀 확인. 핵심 응답/선택기 PNG 직접 확인.
- 독립 source review: stale focused pane, wrong tmux owner, nested CLI, disconnected Herdr guard 수리 후 PASS.
- Diff mapping: 55 files / 132 hunks / unmapped0

## 검증 한계

UI/transport E2E는 실제 PTY에서 결정적 Claude/Codex 형식 테스트 CLI를 실행했다. 유료 모델 API 호출·모델 품질·실제 계정별 모델 접근권한은 검증하지 않았다. iOS 기기 QA는 Firebase/Xcode toolchain 문제로 미완료다. Zellij 정확한 OS PID 및 원격 desktop tmux 자동 연결은 미지원이며 차단한다. GitHub의 기존 Ubuntu lock 경쟁 실패는 최신 push CI 결과와 별도로 표시한다.
