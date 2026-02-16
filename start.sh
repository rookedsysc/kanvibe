#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"

# .env 파일에서 환경변수 로드
if [ -f .env ]; then
  export $(grep -v '^#' .env | grep -v '^$' | xargs)
fi

echo "=== KanVibe 시작 ==="

# 의존성 설치
echo "[1/6] 의존성 설치..."
pnpm install

# node-pty spawn-helper 실행 권한 복구
echo "[1.5/6] spawn-helper 권한 복구..."
chmod +x node_modules/node-pty/prebuilds/darwin-arm64/spawn-helper 2>/dev/null || true

# PostgreSQL Docker 컨테이너 시작
echo "[2/6] PostgreSQL 시작..."
docker compose up -d db

# DB 준비 대기
echo "[3/6] DB 준비 대기..."
until docker compose exec db pg_isready -U "${KANVIBE_USER:-admin}" -q 2>/dev/null; do
  sleep 1
done
echo "       DB 준비 완료"

# DB 마이그레이션 실행
echo "[4/6] DB 마이그레이션 실행..."
pnpm migration:run

# Next.js 빌드
echo "[5/6] Next.js 빌드..."
export NODE_ENV=production
pnpm build

# 앱 시작 (production 모드로 실행하여 auto reload 비활성화)
echo "[6/6] KanVibe 서버 시작..."
exec pnpm start
