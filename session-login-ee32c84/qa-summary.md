# 로그인·사용량 통합 QA

검증 커밋: ee32c847c8a658e28623085a8a964b64a91350f0

- Node 24.22.1 / pnpm 10.17.0
- pnpm check: exit 0
- pnpm test: exit 0, 263 files / 3740 tests
- pnpm build: exit 0
- 실제 Electron + node-pty + xterm, 테스트용 CLI와 계정/사용량 응답 사용
- 로그인 실패 출력 유지, 닫기/재시도, 성공 후 목록·사용량 갱신
- 계정 추가/제거, 종료 전후 resize IPC 200회, 1440/768/390px 가로 넘침 없음
- 실제 Claude OAuth 완료 및 설치 앱 검증 미실행
- 미추적 orca 프로젝트를 제외한 격리 체크아웃에서 동일한 변경 지문으로 CI 명령을 검증
