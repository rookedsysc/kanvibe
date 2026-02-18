#!/bin/bash
set -e

# ─────────────────────────────────────────────────────────────
# KanVibe CLI
# 사용법: bash kanvibe start | bash kanvibe stop
# ─────────────────────────────────────────────────────────────

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"
PID_FILE="$SCRIPT_DIR/.kanvibe.pid"

# ── 색상 및 아이콘 ───────────────────────────────────────────
BOLD='\033[1m'
DIM='\033[2m'
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color
CHECK="${GREEN}✓${NC}"
CROSS="${RED}✗${NC}"
ARROW="${CYAN}→${NC}"
WARN="${YELLOW}!${NC}"

# ── 로케일 감지 ─────────────────────────────────────────────
detect_locale() {
  local lang_env="${LANG:-${LC_ALL:-en_US.UTF-8}}"
  case "$lang_env" in
    ko*) echo "ko" ;;
    zh*) echo "zh" ;;
    *)   echo "en" ;;
  esac
}

LOCALE=$(detect_locale)

# ── i18n 메시지 ─────────────────────────────────────────────
msg() {
  local key="$1"
  shift
  local text=""

  case "${LOCALE}:${key}" in
    # ── 일반 ──
    ko:title)           text="KanVibe" ;;
    en:title)           text="KanVibe" ;;
    zh:title)           text="KanVibe" ;;

    ko:usage)           text="사용법: bash kanvibe.sh {start|stop}" ;;
    en:usage)           text="Usage: bash kanvibe.sh {start|stop}" ;;
    zh:usage)           text="用法: bash kanvibe.sh {start|stop}" ;;

    ko:unknown_cmd)     text="알 수 없는 명령: $1" ;;
    en:unknown_cmd)     text="Unknown command: $1" ;;
    zh:unknown_cmd)     text="未知命令: $1" ;;

    # ── 의존성 체크 ──
    ko:checking_deps)   text="의존성 확인 중..." ;;
    en:checking_deps)   text="Checking dependencies..." ;;
    zh:checking_deps)   text="检查依赖项..." ;;

    ko:dep_found)       text="$1 ${DIM}($2)${NC}" ;;
    en:dep_found)       text="$1 ${DIM}($2)${NC}" ;;
    zh:dep_found)       text="$1 ${DIM}($2)${NC}" ;;

    ko:dep_missing)     text="$1 — 설치되지 않음" ;;
    en:dep_missing)     text="$1 — not installed" ;;
    zh:dep_missing)     text="$1 — 未安装" ;;

    ko:dep_old)         text="$1 — 버전 $2 (최소 $3 필요)" ;;
    en:dep_old)         text="$1 — version $2 (requires $3+)" ;;
    zh:dep_old)         text="$1 — 版本 $2 (需要 $3+)" ;;

    ko:all_deps_ok)     text="모든 의존성이 준비되었습니다" ;;
    en:all_deps_ok)     text="All dependencies are ready" ;;
    zh:all_deps_ok)     text="所有依赖项已就绪" ;;

    ko:deps_missing)    text="필수 의존성이 누락되었습니다" ;;
    en:deps_missing)    text="Required dependencies are missing" ;;
    zh:deps_missing)    text="缺少必需的依赖项" ;;

    # ── 설치 프롬프트 ──
    ko:install_prompt)  text="$1 을(를) 설치하시겠습니까?" ;;
    en:install_prompt)  text="Install $1?" ;;
    zh:install_prompt)  text="是否安装 $1?" ;;

    ko:yn)              text="[Y/n]" ;;
    en:yn)              text="[Y/n]" ;;
    zh:yn)              text="[Y/n]" ;;

    ko:installing)      text="$1 설치 중..." ;;
    en:installing)      text="Installing $1..." ;;
    zh:installing)      text="正在安装 $1..." ;;

    ko:install_ok)      text="$1 설치 완료" ;;
    en:install_ok)      text="$1 installed successfully" ;;
    zh:install_ok)      text="$1 安装成功" ;;

    ko:install_fail)    text="$1 설치 실패" ;;
    en:install_fail)    text="$1 installation failed" ;;
    zh:install_fail)    text="$1 安装失败" ;;

    ko:install_skip)    text="$1 설치를 건너뜁니다" ;;
    en:install_skip)    text="Skipping $1 installation" ;;
    zh:install_skip)    text="跳过安装 $1" ;;

    ko:brew_missing)    text="Homebrew가 설치되어 있지 않습니다. https://brew.sh 에서 설치해주세요." ;;
    en:brew_missing)    text="Homebrew is not installed. Please install from https://brew.sh" ;;
    zh:brew_missing)    text="未安装 Homebrew。请从 https://brew.sh 安装。" ;;

    ko:gh_not_authed)   text="gh가 인증되지 않았습니다. 'gh auth login'을 실행해주세요.\n         인증 없이는 PR URL 자동 감지 및 gh 기반 커밋 기능을 사용할 수 없습니다." ;;
    en:gh_not_authed)   text="gh is not authenticated. Please run 'gh auth login'.\n         Without authentication, PR URL detection and gh-based commit features won't work." ;;
    zh:gh_not_authed)   text="gh 未认证。请运行 'gh auth login'。\n         未认证将无法使用 PR URL 自动检测和基于 gh 的提交功能。" ;;

    ko:optional_label)  text="선택" ;;
    en:optional_label)  text="optional" ;;
    zh:optional_label)  text="可选" ;;

    ko:tmux_conf_prompt) text="tmux 추천 설정을 설치하시겠습니까? (마우스 스크롤, 세션 복원 등)" ;;
    en:tmux_conf_prompt) text="Install recommended tmux config? (mouse scroll, session restore, etc.)" ;;
    zh:tmux_conf_prompt) text="是否安装推荐的 tmux 配置？（鼠标滚动、会话恢复等）" ;;

    ko:tmux_conf_installed) text="tmux 설정 파일이 설치되었습니다 (~/.tmux.conf)" ;;
    en:tmux_conf_installed) text="tmux config installed (~/.tmux.conf)" ;;
    zh:tmux_conf_installed) text="tmux 配置已安装 (~/.tmux.conf)" ;;

    ko:tmux_conf_skip) text="tmux 설정 설치를 건너뜁니다" ;;
    en:tmux_conf_skip) text="Skipping tmux config installation" ;;
    zh:tmux_conf_skip) text="跳过 tmux 配置安装" ;;

    ko:tmux_conf_backup) text="기존 ~/.tmux.conf를 ~/.tmux.conf.bak으로 백업했습니다" ;;
    en:tmux_conf_backup) text="Backed up existing ~/.tmux.conf to ~/.tmux.conf.bak" ;;
    zh:tmux_conf_backup) text="已将现有 ~/.tmux.conf 备份为 ~/.tmux.conf.bak" ;;

    ko:tmux_tpm_installed) text="TPM (Tmux Plugin Manager) 설치 완료" ;;
    en:tmux_tpm_installed) text="TPM (Tmux Plugin Manager) installed" ;;
    zh:tmux_tpm_installed) text="TPM（Tmux 插件管理器）已安装" ;;

    # ── start 단계 ──
    ko:starting)        text="KanVibe 시작" ;;
    en:starting)        text="Starting KanVibe" ;;
    zh:starting)        text="启动 KanVibe" ;;

    ko:step_deps)       text="의존성 설치" ;;
    en:step_deps)       text="Installing dependencies" ;;
    zh:step_deps)       text="安装依赖" ;;

    ko:step_db)         text="PostgreSQL 시작" ;;
    en:step_db)         text="Starting PostgreSQL" ;;
    zh:step_db)         text="启动 PostgreSQL" ;;

    ko:step_db_wait)    text="DB 준비 대기" ;;
    en:step_db_wait)    text="Waiting for DB" ;;
    zh:step_db_wait)    text="等待数据库就绪" ;;

    ko:step_db_ready)   text="DB 준비 완료" ;;
    en:step_db_ready)   text="DB is ready" ;;
    zh:step_db_ready)   text="数据库已就绪" ;;

    ko:step_migrate)    text="DB 마이그레이션 실행" ;;
    en:step_migrate)    text="Running DB migrations" ;;
    zh:step_migrate)    text="执行数据库迁移" ;;

    ko:step_build)      text="Next.js 빌드" ;;
    en:step_build)      text="Building Next.js" ;;
    zh:step_build)      text="构建 Next.js" ;;

    ko:step_server)     text="서버 시작" ;;
    en:step_server)     text="Starting server" ;;
    zh:step_server)     text="启动服务器" ;;

    ko:already_running) text="KanVibe가 이미 실행 중입니다 (PID: $1)" ;;
    en:already_running) text="KanVibe is already running (PID: $1)" ;;
    zh:already_running) text="KanVibe 已在运行 (PID: $1)" ;;

    # ── stop 단계 ──
    ko:stopping)        text="KanVibe 종료" ;;
    en:stopping)        text="Stopping KanVibe" ;;
    zh:stopping)        text="停止 KanVibe" ;;

    ko:stop_app)        text="앱 프로세스 종료" ;;
    en:stop_app)        text="Stopping app process" ;;
    zh:stop_app)        text="停止应用进程" ;;

    ko:stop_db)         text="PostgreSQL 종료" ;;
    en:stop_db)         text="Stopping PostgreSQL" ;;
    zh:stop_db)         text="停止 PostgreSQL" ;;

    ko:stopped)         text="KanVibe가 종료되었습니다" ;;
    en:stopped)         text="KanVibe has been stopped" ;;
    zh:stopped)         text="KanVibe 已停止" ;;

    ko:not_running)     text="KanVibe가 실행 중이 아닙니다" ;;
    en:not_running)     text="KanVibe is not running" ;;
    zh:not_running)     text="KanVibe 未在运行" ;;

    ko:run_mode_prompt) text="실행 모드를 선택하세요:" ;;
    en:run_mode_prompt) text="Select run mode:" ;;
    zh:run_mode_prompt) text="选择运行模式:" ;;

    ko:run_fg)          text="포그라운드 (터미널에 직접 출력, Ctrl+C로 종료)" ;;
    en:run_fg)          text="Foreground (output to terminal, Ctrl+C to stop)" ;;
    zh:run_fg)          text="前台运行 (输出到终端, Ctrl+C 停止)" ;;

    ko:run_bg)          text="백그라운드 (터미널 닫아도 서버 유지)" ;;
    en:run_bg)          text="Background (server keeps running after terminal closes)" ;;
    zh:run_bg)          text="后台运行 (关闭终端后服务器继续运行)" ;;

    ko:run_bg_started)  text="KanVibe가 백그라운드에서 시작되었습니다 (PID: $1)" ;;
    en:run_bg_started)  text="KanVibe started in background (PID: $1)" ;;
    zh:run_bg_started)  text="KanVibe 已在后台启动 (PID: $1)" ;;

    ko:run_bg_log)      text="로그: $1" ;;
    en:run_bg_log)      text="Log: $1" ;;
    zh:run_bg_log)      text="日志: $1" ;;

    ko:done)            text="완료" ;;
    en:done)            text="Done" ;;
    zh:done)            text="完成" ;;

    *) text="[$key]" ;;
  esac

  printf "%b" "$text"
}

# ── 유틸리티 ─────────────────────────────────────────────────
print_header() {
  echo ""
  printf "  ${BOLD}${BLUE}╔══════════════════════════════════════╗${NC}\n"
  printf "  ${BOLD}${BLUE}║${NC}  ${BOLD}$(msg title)${NC}                          ${BOLD}${BLUE}║${NC}\n"
  printf "  ${BOLD}${BLUE}╚══════════════════════════════════════╝${NC}\n"
  echo ""
}

# 단계별 진행 출력
step() {
  local current="$1"
  local total="$2"
  local message="$3"
  printf "  ${BOLD}${BLUE}[%d/%d]${NC} %b\n" "$current" "$total" "$message"
}

step_done() {
  local current="$1"
  local total="$2"
  local message="$3"
  printf "  ${CHECK} ${DIM}[%d/%d]${NC} %b\n" "$current" "$total" "$message"
}

# ── .env 로드 ────────────────────────────────────────────────
load_env() {
  if [ -f .env ]; then
    export $(grep -v '^#' .env | grep -v '^$' | xargs)
  fi
}

# ── 의존성 체크 ─────────────────────────────────────────────
# 버전 비교: $1 >= $2 이면 0 반환
version_gte() {
  local v1="$1" v2="$2"
  # sort -V로 정렬 후 마지막이 v1이면 v1 >= v2
  [ "$(printf '%s\n%s' "$v2" "$v1" | sort -V | tail -n1)" = "$v1" ]
}

# 개별 의존성 체크 결과를 저장할 배열
MISSING_REQUIRED=()
MISSING_OPTIONAL=()

check_single_dep() {
  local name="$1"
  local cmd="$2"
  local required="$3"        # "required" 또는 "optional"
  local min_version="$4"     # 최소 버전 (빈 문자열이면 버전 체크 안 함)
  local install_method="$5"  # brew install 명령

  if ! command -v "$cmd" &>/dev/null; then
    if [ "$required" = "required" ]; then
      printf "  ${CROSS} %b\n" "$(msg dep_missing "$name")"
      MISSING_REQUIRED+=("$name|$install_method")
    else
      printf "  ${WARN} %b ${DIM}($(msg optional_label))${NC}\n" "$(msg dep_missing "$name")"
      MISSING_OPTIONAL+=("$name|$install_method")
    fi
    return 1
  fi

  # 버전 체크
  local version=""
  case "$cmd" in
    node)   version=$(node -v 2>/dev/null | sed 's/^v//') ;;
    pnpm)   version=$(pnpm -v 2>/dev/null) ;;
    docker) version=$(docker --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1) ;;
    git)    version=$(git --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1) ;;
    tmux)   version=$(tmux -V 2>/dev/null | grep -oE '[0-9]+\.[0-9]+[a-z]?' | head -1) ;;
    zellij) version=$(zellij --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1) ;;
    gh)     version=$(gh --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1) ;;
  esac

  if [ -n "$min_version" ] && [ -n "$version" ]; then
    if ! version_gte "$version" "$min_version"; then
      if [ "$required" = "required" ]; then
        printf "  ${CROSS} %b\n" "$(msg dep_old "$name" "$version" "$min_version")"
        MISSING_REQUIRED+=("$name|$install_method")
      else
        printf "  ${WARN} %b ${DIM}($(msg optional_label))${NC}\n" "$(msg dep_old "$name" "$version" "$min_version")"
        MISSING_OPTIONAL+=("$name|$install_method")
      fi
      return 1
    fi
  fi

  local display_version="${version:-"?"}"
  if [ "$required" = "optional" ]; then
    printf "  ${CHECK} %b ${DIM}($(msg optional_label))${NC}\n" "$(msg dep_found "$name" "$display_version")"
  else
    printf "  ${CHECK} %b\n" "$(msg dep_found "$name" "$display_version")"
  fi
  return 0
}

check_deps() {
  printf "\n  ${BOLD}$(msg checking_deps)${NC}\n\n"

  MISSING_REQUIRED=()
  MISSING_OPTIONAL=()

  # 필수 의존성
  check_single_dep "Node.js"  "node"   "required" "22.0.0" "brew install node" || true
  check_single_dep "pnpm"     "pnpm"   "required" ""       "corepack_pnpm"     || true
  check_single_dep "Docker"   "docker" "required" ""       "brew install --cask docker" || true
  check_single_dep "git"      "git"    "required" ""       "brew install git"  || true
  check_single_dep "tmux"     "tmux"   "required" ""       "brew install tmux" || true
  check_single_dep "gh"       "gh"     "required" ""       "brew install gh"   || true

  # gh 인증 상태 체크
  if command -v gh &>/dev/null; then
    if gh auth status &>/dev/null; then
      printf "  ${CHECK} gh auth ${DIM}(authenticated)${NC}\n"
    else
      printf "  ${WARN} $(msg gh_not_authed)\n"
    fi
  fi

  # 선택 의존성 — tmux가 이미 있으면 zellij 설치 프롬프트 생략
  if ! command -v tmux &>/dev/null; then
    check_single_dep "zellij"   "zellij" "optional" ""       "brew install zellij" || true
  fi

  echo ""
}

# ── 의존성 설치 ─────────────────────────────────────────────
confirm_install() {
  local name="$1"
  printf "  ${ARROW} $(msg install_prompt "$name") $(msg yn) "
  read -r answer
  case "$answer" in
    [Nn]*) return 1 ;;
    *)     return 0 ;;
  esac
}

install_single_dep() {
  local name="$1"
  local method="$2"

  if ! confirm_install "$name"; then
    printf "  ${DIM}  $(msg install_skip "$name")${NC}\n"
    return 1
  fi

  printf "  ${ARROW} $(msg installing "$name")\n"

  if [ "$method" = "corepack_pnpm" ]; then
    # pnpm 설치: corepack 우선, 없으면 npm 글로벌 설치로 fallback
    if command -v corepack &>/dev/null; then
      if corepack enable && corepack prepare pnpm@latest --activate 2>/dev/null; then
        printf "  ${CHECK} $(msg install_ok "$name")\n"
        return 0
      fi
    fi
    # corepack 없거나 실패 시 npm으로 설치
    if command -v npm &>/dev/null && npm install -g pnpm 2>/dev/null; then
      printf "  ${CHECK} $(msg install_ok "$name")\n"
      return 0
    fi
    printf "  ${CROSS} $(msg install_fail "$name")\n"
    return 1
  fi

  # Homebrew 기반 설치
  if ! command -v brew &>/dev/null; then
    printf "  ${CROSS} $(msg brew_missing)\n"
    return 1
  fi

  if eval "$method" 2>/dev/null; then
    printf "  ${CHECK} $(msg install_ok "$name")\n"
    return 0
  else
    printf "  ${CROSS} $(msg install_fail "$name")\n"
    return 1
  fi
}

install_missing_deps() {
  local has_failure=0

  # 필수 의존성 설치
  for entry in "${MISSING_REQUIRED[@]}"; do
    local name="${entry%%|*}"
    local method="${entry##*|}"
    echo ""
    if ! install_single_dep "$name" "$method"; then
      has_failure=1
    fi
  done

  # 선택 의존성 설치
  for entry in "${MISSING_OPTIONAL[@]}"; do
    local name="${entry%%|*}"
    local method="${entry##*|}"
    echo ""
    if ! install_single_dep "$name" "$method"; then
      : # 선택 의존성 실패는 무시
    fi
  done

  # 필수 의존성 재확인
  if [ "$has_failure" -eq 1 ]; then
    echo ""
    printf "  ${CROSS} $(msg deps_missing)\n\n"
    exit 1
  fi

  # 필수 의존성이 있었으면 재확인
  if [ "${#MISSING_REQUIRED[@]}" -gt 0 ]; then
    MISSING_REQUIRED=()
    check_single_dep "Node.js"  "node"   "required" "22.0.0" "" || true
    check_single_dep "pnpm"     "pnpm"   "required" ""       "" || true
    check_single_dep "Docker"   "docker" "required" ""       "" || true
    check_single_dep "git"      "git"    "required" ""       "" || true
    check_single_dep "tmux"     "tmux"   "required" ""       "" || true
    check_single_dep "gh"       "gh"     "required" ""       "" || true

    if [ "${#MISSING_REQUIRED[@]}" -gt 0 ]; then
      echo ""
      printf "  ${CROSS} $(msg deps_missing)\n\n"
      exit 1
    fi
  fi
}

# ── tmux 설정 파일 설치 ──────────────────────────────────────
TMUX_CONF_URL="https://raw.githubusercontent.com/rookedsysc/dotfiles/master/configs/tmux/.tmux.conf"

setup_tmux_conf() {
  # tmux가 없으면 스킵
  command -v tmux &>/dev/null || return 0
  # 이미 .tmux.conf가 있으면 스킵
  [ -f "$HOME/.tmux.conf" ] && return 0

  echo ""
  printf "  ${ARROW} $(msg tmux_conf_prompt) $(msg yn) "
  read -r answer
  case "$answer" in
    [Nn]*)
      printf "  ${DIM}  $(msg tmux_conf_skip)${NC}\n"
      return 0
      ;;
  esac

  # 기존 파일 백업
  if [ -f "$HOME/.tmux.conf" ]; then
    cp "$HOME/.tmux.conf" "$HOME/.tmux.conf.bak"
    printf "  ${WARN} $(msg tmux_conf_backup)\n"
  fi

  # 설정 파일 다운로드
  if command -v curl &>/dev/null; then
    curl -fsSL "$TMUX_CONF_URL" -o "$HOME/.tmux.conf"
  elif command -v wget &>/dev/null; then
    wget -qO "$HOME/.tmux.conf" "$TMUX_CONF_URL"
  else
    printf "  ${CROSS} curl/wget not found\n"
    return 1
  fi

  # TPM 설치
  if [ ! -d "$HOME/.tmux/plugins/tpm" ]; then
    git clone https://github.com/tmux-plugins/tpm "$HOME/.tmux/plugins/tpm" 2>/dev/null
    printf "  ${CHECK} $(msg tmux_tpm_installed)\n"
  fi

  printf "  ${CHECK} $(msg tmux_conf_installed)\n"
}

# ── start 커맨드 ─────────────────────────────────────────────
cmd_start() {
  print_header
  load_env

  # 이미 실행 중인지 확인
  if [ -f "$PID_FILE" ]; then
    local pid
    pid=$(cat "$PID_FILE")
    if kill -0 "$pid" 2>/dev/null; then
      printf "  ${WARN} $(msg already_running "$pid")\n\n"
      exit 1
    fi
    rm -f "$PID_FILE"
  fi

  # 의존성 체크
  check_deps

  # 미설치 항목이 있으면 설치 진행
  if [ "${#MISSING_REQUIRED[@]}" -gt 0 ] || [ "${#MISSING_OPTIONAL[@]}" -gt 0 ]; then
    install_missing_deps
    echo ""
  else
    printf "  ${CHECK} $(msg all_deps_ok)\n"
  fi

  # tmux 설정 파일 설치 (최초 1회)
  setup_tmux_conf

  local total=6
  printf "\n  ${BOLD}═══ $(msg starting) ═══${NC}\n\n"

  # 1. pnpm install
  step 1 $total "$(msg step_deps)"
  pnpm install --reporter=silent 2>&1 | tail -1 || pnpm install
  step_done 1 $total "$(msg step_deps)"

  # 2. PostgreSQL 시작
  step 2 $total "$(msg step_db)"
  docker compose up -d db 2>&1 | tail -1
  step_done 2 $total "$(msg step_db)"

  # 3. DB 대기
  step 3 $total "$(msg step_db_wait)"
  until docker compose exec db pg_isready -U "${KANVIBE_USER:-admin}" -q 2>/dev/null; do
    sleep 1
  done
  step_done 3 $total "$(msg step_db_ready)"

  # 4. 마이그레이션
  step 4 $total "$(msg step_migrate)"
  pnpm migration:run 2>&1 | tail -3
  step_done 4 $total "$(msg step_migrate)"

  # 5. Next.js 빌드
  step 5 $total "$(msg step_build)"
  export NODE_ENV=production
  pnpm build 2>&1 | tail -3
  step_done 5 $total "$(msg step_build)"

  # 6. 서버 시작 — 실행 모드 선택
  step 6 $total "$(msg step_server)"
  echo ""
  printf "  $(msg run_mode_prompt)\n"
  printf "    ${BOLD}1)${NC} $(msg run_fg)\n"
  printf "    ${BOLD}2)${NC} $(msg run_bg)\n"
  printf "  ${ARROW} [1/2] "
  read -r run_mode

  local LOG_FILE="$SCRIPT_DIR/logs/kanvibe.log"

  case "${run_mode:-1}" in
    2)
      # 백그라운드 실행
      mkdir -p "$SCRIPT_DIR/logs"
      nohup pnpm start > "$LOG_FILE" 2>&1 &
      local app_pid=$!
      echo "$app_pid" > "$PID_FILE"
      echo ""
      printf "  ${CHECK} $(msg run_bg_started "$app_pid")\n"
      printf "  ${DIM}  $(msg run_bg_log "$LOG_FILE")${NC}\n"
      printf "  ${DIM}  bash kanvibe.sh stop${NC}\n\n"
      ;;
    *)
      # 포그라운드 실행
      pnpm start &
      local app_pid=$!
      echo "$app_pid" > "$PID_FILE"
      wait "$app_pid"
      ;;
  esac
}

# ── stop 커맨드 ──────────────────────────────────────────────
cmd_stop() {
  print_header
  load_env

  local total=2
  printf "  ${BOLD}═══ $(msg stopping) ═══${NC}\n\n"

  # 1. 앱 프로세스 종료
  step 1 $total "$(msg stop_app)"
  if [ -f "$PID_FILE" ]; then
    local pid
    pid=$(cat "$PID_FILE")
    if kill -0 "$pid" 2>/dev/null; then
      kill "$pid" 2>/dev/null || true
      # 프로세스 종료 대기 (최대 10초)
      local count=0
      while kill -0 "$pid" 2>/dev/null && [ "$count" -lt 10 ]; do
        sleep 1
        count=$((count + 1))
      done
      # 아직 살아있으면 강제 종료
      if kill -0 "$pid" 2>/dev/null; then
        kill -9 "$pid" 2>/dev/null || true
      fi
    fi
    rm -f "$PID_FILE"
    step_done 1 $total "$(msg stop_app)"
  else
    printf "  ${DIM}  $(msg not_running)${NC}\n"
    step_done 1 $total "$(msg stop_app)"
  fi

  # 2. Docker DB 종료
  step 2 $total "$(msg stop_db)"
  docker compose down 2>&1 | tail -1
  step_done 2 $total "$(msg stop_db)"

  echo ""
  printf "  ${CHECK} ${BOLD}$(msg stopped)${NC}\n\n"
}

# ── 메인 ─────────────────────────────────────────────────────
case "${1:-}" in
  start) cmd_start ;;
  stop)  cmd_stop ;;
  *)
    print_header
    printf "  $(msg usage)\n\n"
    printf "  ${BOLD}start${NC}   $(msg starting)\n"
    printf "  ${BOLD}stop${NC}    $(msg stopping)\n\n"
    exit 1
    ;;
esac
