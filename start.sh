#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"

echo "=== KanVibe 시작 ==="

# 의존성 설치
echo "[1/6] 의존성 설치..."
npm install

# PostgreSQL Docker 컨테이너 시작
echo "[2/6] PostgreSQL 시작..."
docker compose up -d db

# DB 준비 대기
echo "[3/6] DB 준비 대기..."
until docker compose exec db pg_isready -U kanvibe -q 2>/dev/null; do
  sleep 1
done
echo "       DB 준비 완료"

# DB 마이그레이션 실행
echo "[4/6] DB 마이그레이션 실행..."
npm run migration:run

# Next.js 빌드
echo "[5/6] Next.js 빌드..."
export NODE_ENV=production
npm run build

# 앱 시작 (production 모드로 실행하여 auto reload 비활성화)
echo "[6/6] KanVibe 서버 시작..."
exec npm run start
