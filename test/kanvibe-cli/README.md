# KanVibe CLI Test Guide

`kanvibe.sh` 스크립트를 Docker 환경에서 테스트하는 가이드.

## 사전 요구사항

- Docker Desktop 실행 중

## 테스트 시나리오

### 1. 의존성 누락 환경 테스트 (Dockerfile.bare)

의존성이 대부분 없는 클린 Ubuntu에서 체크 UI가 올바르게 동작하는지 확인.

```bash
# 빌드
docker build -t kanvibe-cli-bare -f test/kanvibe-cli/Dockerfile.bare .

# 영어 (기본)
docker run --rm -it kanvibe-cli-bare bash kanvibe.sh

# 한국어
docker run --rm -it -e LANG=ko_KR.UTF-8 kanvibe-cli-bare bash kanvibe.sh

# 중국어
docker run --rm -it -e LANG=zh_CN.UTF-8 kanvibe-cli-bare bash kanvibe.sh

# start 실행 (의존성 누락 에러 확인)
docker run --rm -it kanvibe-cli-bare bash kanvibe.sh start
```

**확인 사항:**
- [ ] usage 메시지가 로케일에 맞게 출력되는가
- [ ] git만 ✓, 나머지(Node.js, pnpm, Docker, tmux, gh)는 ✗로 표시되는가
- [ ] zellij가 `!` (optional)로 표시되는가
- [ ] start 시 필수 의존성 누락 에러로 종료되는가

---

### 2. 모든 의존성 설치 환경 테스트 (Dockerfile)

모든 필수 의존성이 설치된 환경에서 체크 UI 확인.

```bash
# 빌드
docker build -t kanvibe-cli-test -f test/kanvibe-cli/Dockerfile .

# 의존성 체크 확인
docker run --rm -it kanvibe-cli-test bash kanvibe.sh

# start 의존성 체크만 확인 (Ctrl+C로 중단)
docker run --rm -it kanvibe-cli-test bash kanvibe.sh start
```

**확인 사항:**
- [ ] 모든 필수 의존성이 ✓로 표시되는가
- [ ] gh auth 미인증 경고가 표시되는가
- [ ] zellij가 ✗ (optional)로 표시되는가
- [ ] "모든 의존성이 준비되었습니다" 메시지가 출력되는가

---

### 3. 풀 통합 테스트 — start/stop (Docker 소켓 마운트)

호스트 Docker 소켓을 마운트하여 실제 PostgreSQL 컨테이너를 띄우고 전체 start/stop 플로우를 테스트.

```bash
# 빌드
docker build -t kanvibe-cli-test -f test/kanvibe-cli/Dockerfile .

# start (포그라운드 모드 — 1 선택)
docker run --rm -it \
  -v /var/run/docker.sock:/var/run/docker.sock \
  -p 4885:4885 \
  kanvibe-cli-test bash kanvibe.sh start

# start (백그라운드 모드 — 2 선택 후 stop 테스트)
docker run --rm -it \
  -v /var/run/docker.sock:/var/run/docker.sock \
  -p 4885:4885 \
  kanvibe-cli-test bash -c "bash kanvibe.sh start && sleep 5 && bash kanvibe.sh stop"
```

**확인 사항:**
- [ ] pnpm install이 정상 실행되는가
- [ ] PostgreSQL Docker 컨테이너가 시작되는가
- [ ] DB 마이그레이션이 성공하는가
- [ ] Next.js 빌드가 성공하는가
- [ ] 포그라운드/백그라운드 선택 프롬프트가 나타나는가
- [ ] 서버가 `http://localhost:4885`에서 응답하는가
- [ ] stop이 앱 + DB를 모두 종료하는가

---

### 4. i18n 전체 테스트

모든 로케일에서 메시지가 올바르게 출력되는지 한번에 확인.

```bash
docker build -t kanvibe-cli-bare -f test/kanvibe-cli/Dockerfile.bare .

# 3개 로케일 순회 테스트
for lang in en_US.UTF-8 ko_KR.UTF-8 zh_CN.UTF-8; do
  echo "=== $lang ==="
  docker run --rm -e LANG=$lang kanvibe-cli-bare bash kanvibe.sh 2>&1
  echo ""
done
```

**확인 사항:**
- [ ] en: "Usage: bash kanvibe.sh {start|stop}"
- [ ] ko: "사용법: bash kanvibe.sh {start|stop}"
- [ ] zh: "用法: bash kanvibe.sh {start|stop}"

---

### 5. stop 동작 테스트 (서버 미실행 상태)

```bash
docker run --rm -it kanvibe-cli-test bash kanvibe.sh stop
```

**확인 사항:**
- [ ] "KanVibe is not running" 메시지가 출력되는가
- [ ] docker compose down이 실행되는가
- [ ] "KanVibe has been stopped" 메시지가 출력되는가

---

## 정리

테스트 후 Docker 이미지 삭제:

```bash
docker rmi kanvibe-cli-test kanvibe-cli-bare 2>/dev/null
```
