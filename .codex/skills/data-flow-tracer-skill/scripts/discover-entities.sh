#!/usr/bin/env bash
# DB Entity/Model Discovery Script
# 프로젝트에서 ORM 엔티티, DTO, Mapper 파일을 자동 탐지하여 초기 분석 자료를 제공한다.
#
# Usage: ./discover-entities.sh [project-root] [target-table-or-entity]
# Arguments:
#   project-root: 프로젝트 루트 경로 (기본: 현재 디렉토리)
#   target-table-or-entity: 특정 테이블/엔티티명으로 범위 제한 (선택)

set -euo pipefail

PROJECT_ROOT="${1:-.}"
TARGET="${2:-}"

# 색상 정의
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m'

header() { echo -e "\n${BLUE}══════════════════════════════════════${NC}"; echo -e "${CYAN}$1${NC}"; echo -e "${BLUE}══════════════════════════════════════${NC}"; }
section() { echo -e "\n${GREEN}▶ $1${NC}"; }
warn() { echo -e "${YELLOW}  ⚠ $1${NC}"; }
info() { echo -e "  $1"; }

cd "$PROJECT_ROOT"

header "ORM Framework Detection"

DETECTED_FRAMEWORKS=()

# JPA / Hibernate (Java/Kotlin)
if rg -l '@Entity|@Table' --type java --type kotlin 2>/dev/null | head -1 > /dev/null 2>&1; then
  DETECTED_FRAMEWORKS+=("JPA")
  info "✅ JPA/Hibernate detected (Java/Kotlin)"
fi

# TypeORM (TypeScript)
if rg -l '@Entity\(\)|@Column\(\)' --type ts 2>/dev/null | head -1 > /dev/null 2>&1; then
  DETECTED_FRAMEWORKS+=("TypeORM")
  info "✅ TypeORM detected (TypeScript)"
fi

# Prisma
if find . -name "schema.prisma" -not -path "*/node_modules/*" 2>/dev/null | head -1 | grep -q .; then
  DETECTED_FRAMEWORKS+=("Prisma")
  info "✅ Prisma detected"
fi

# SQLAlchemy (Python)
if rg -l 'declarative_base|mapped_column|Column\(' --type py 2>/dev/null | head -1 > /dev/null 2>&1; then
  DETECTED_FRAMEWORKS+=("SQLAlchemy")
  info "✅ SQLAlchemy detected (Python)"
fi

# Django (Python)
if rg -l 'models\.Model|class Meta:' --type py 2>/dev/null | head -1 > /dev/null 2>&1; then
  DETECTED_FRAMEWORKS+=("Django")
  info "✅ Django detected (Python)"
fi

# Sequelize (JavaScript/TypeScript)
if rg -l 'sequelize\.define|Model\.init|@Table' --type js --type ts 2>/dev/null | head -1 > /dev/null 2>&1; then
  DETECTED_FRAMEWORKS+=("Sequelize")
  info "✅ Sequelize detected"
fi

# MyBatis (Java)
if rg -l '@Mapper|@Select|@Insert' --type java 2>/dev/null | head -1 > /dev/null 2>&1 || find . -name "*.xml" -path "*/mapper*" 2>/dev/null | head -1 | grep -q .; then
  DETECTED_FRAMEWORKS+=("MyBatis")
  info "✅ MyBatis detected"
fi

# GORM (Go)
if rg -l 'gorm\.Model|gorm:"' --type go 2>/dev/null | head -1 > /dev/null 2>&1; then
  DETECTED_FRAMEWORKS+=("GORM")
  info "✅ GORM detected (Go)"
fi

if [ ${#DETECTED_FRAMEWORKS[@]} -eq 0 ]; then
  warn "ORM 프레임워크를 감지하지 못했습니다. 수동 분석이 필요합니다."
fi

# ─── Entity/Model 파일 목록 ───
header "Entity / Model Files"

TARGET_PATTERN=""
if [ -n "$TARGET" ]; then
  TARGET_PATTERN="$TARGET"
  info "🎯 필터: $TARGET"
fi

section "JPA Entities (@Entity, @Table)"
if [ -n "$TARGET_PATTERN" ]; then
  rg -l "@Table.*name.*=.*\".*${TARGET_PATTERN}.*\"|@Entity" --type java --type kotlin 2>/dev/null | while read -r f; do
    rg "@Table|@Entity|class " "$f" 2>/dev/null | head -3
    echo "  📁 $f"
    echo ""
  done
else
  rg -l '@Entity|@Table' --type java --type kotlin 2>/dev/null | while read -r f; do
    table=$(rg '@Table\s*\(.*name\s*=\s*"([^"]+)"' -o -r '$1' "$f" 2>/dev/null | head -1)
    entity=$(rg 'class\s+(\w+)' -o -r '$1' "$f" 2>/dev/null | head -1)
    if [ -n "$table" ]; then
      info "📁 $f → Table: $table, Class: $entity"
    else
      info "📁 $f → Class: $entity"
    fi
  done
fi

section "TypeORM Entities"
rg -l '@Entity\(\)|@Entity\(' --type ts 2>/dev/null | while read -r f; do
  table=$(rg "@Entity\(['\"]([^'\"]+)" -o -r '$1' "$f" 2>/dev/null | head -1)
  entity=$(rg 'class\s+(\w+)' -o -r '$1' "$f" 2>/dev/null | head -1)
  info "📁 $f → Table: ${table:-auto}, Class: $entity"
done

section "Prisma Models"
find . -name "schema.prisma" -not -path "*/node_modules/*" 2>/dev/null | while read -r f; do
  info "📁 $f"
  if [ -n "$TARGET_PATTERN" ]; then
    rg "model\s+.*${TARGET_PATTERN}|@@map\(\".*${TARGET_PATTERN}" "$f" 2>/dev/null | head -10
  else
    rg 'model\s+\w+|@@map\("' "$f" 2>/dev/null | head -20
  fi
done

section "SQLAlchemy Models"
rg -l '__tablename__|mapped_column' --type py 2>/dev/null | while read -r f; do
  table=$(rg '__tablename__\s*=\s*["\x27]([^"\x27]+)' -o -r '$1' "$f" 2>/dev/null | head -1)
  entity=$(rg 'class\s+(\w+)' -o -r '$1' "$f" 2>/dev/null | head -1)
  info "📁 $f → Table: ${table:-auto}, Class: $entity"
done

section "Django Models"
rg -l 'models\.Model' --type py 2>/dev/null | while read -r f; do
  rg 'class\s+\w+\(.*Model' "$f" 2>/dev/null | while read -r line; do
    info "📁 $f → $line"
  done
done

# ─── DTO 파일 목록 ───
header "DTO / Response / Request Files"

section "Java/Kotlin DTOs"
rg -l 'Dto|DTO|Response|Request|Vo|VO' --type java --type kotlin -g '*Dto*' -g '*DTO*' -g '*Response*' -g '*Request*' -g '*Vo*' 2>/dev/null | head -30 | while read -r f; do
  info "📁 $f"
done

section "TypeScript DTOs"
rg -l 'Dto|DTO|Response|Request' --type ts -g '*dto*' -g '*response*' -g '*request*' 2>/dev/null | head -30 | while read -r f; do
  info "📁 $f"
done

section "Python DTOs / Schemas (Pydantic)"
rg -l 'BaseModel|Schema|Serializer' --type py -g '*schema*' -g '*dto*' -g '*serializer*' 2>/dev/null | head -30 | while read -r f; do
  info "📁 $f"
done

# ─── Mapper / Converter 파일 목록 ───
header "Mapper / Converter Files"

section "MapStruct / Manual Mappers"
rg -l '@Mapper|Mapper|Converter|toDto|toEntity|from.*Entity|to.*Response' --type java --type kotlin --type ts --type py 2>/dev/null \
  | rg -v 'node_modules|\.d\.ts' \
  | head -30 | while read -r f; do
  info "📁 $f"
done

# ─── Column Mapping 샘플 ───
header "Column Mapping Samples (DB name ↔ Field name)"

section "@Column 매핑 (JPA/TypeORM)"
rg '@Column\s*\(.*name\s*=\s*"([^"]+)"' --type java --type kotlin --type ts -n 2>/dev/null | head -20 | while read -r line; do
  info "$line"
done

section "@JsonProperty 매핑"
rg '@JsonProperty\s*\(\s*"([^"]+)"' --type java --type kotlin --type ts -n 2>/dev/null | head -20 | while read -r line; do
  info "$line"
done

section "Prisma @@map / @map 매핑"
find . -name "schema.prisma" -not -path "*/node_modules/*" 2>/dev/null | xargs rg '@map\("|@@map\("' -n 2>/dev/null | head -20 | while read -r line; do
  info "$line"
done

# ─── 요약 ───
header "Summary"
echo -e "${GREEN}감지된 프레임워크:${NC} ${DETECTED_FRAMEWORKS[*]:-없음}"
echo ""
echo "Entity 파일 수: $(rg -l '@Entity|@Table|__tablename__|models\.Model|@Entity\(' --type java --type kotlin --type ts --type py 2>/dev/null | wc -l | tr -d ' ')"
echo "DTO 파일 수: $(rg -l 'Dto|DTO|BaseModel' -g '*dto*' -g '*DTO*' -g '*Dto*' -g '*schema*' --type java --type kotlin --type ts --type py 2>/dev/null | wc -l | tr -d ' ')"
echo "Mapper 파일 수: $(rg -l '@Mapper|Mapper|Converter|toDto|toEntity' --type java --type kotlin --type ts --type py 2>/dev/null | rg -v 'node_modules' | wc -l | tr -d ' ')"
echo ""
info "💡 상세 분석은 Serena의 find_symbol, find_referencing_symbols를 사용하여 진행하세요."
