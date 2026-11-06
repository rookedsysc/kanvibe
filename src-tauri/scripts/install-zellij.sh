if command -v zellij >/dev/null 2>&1; then exit 0; fi
run_install() {
  if [ "$(id -u)" -eq 0 ]; then
    sh -lc "$1"
    return $? 
  fi
  if command -v sudo >/dev/null 2>&1; then
    sudo -n sh -lc "$1"
    return $? 
  fi
  return 1
}
if command -v cargo >/dev/null 2>&1; then cargo install --locked zellij; fi
if command -v brew >/dev/null 2>&1; then brew install zellij; fi
if command -v apt-get >/dev/null 2>&1; then run_install 'apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y zellij' && exit 0; fi
if command -v dnf >/dev/null 2>&1; then run_install 'dnf install -y zellij' && exit 0; fi
if command -v yum >/dev/null 2>&1; then run_install 'yum install -y zellij' && exit 0; fi
if command -v pacman >/dev/null 2>&1; then run_install 'pacman -Sy --noconfirm zellij' && exit 0; fi
if command -v zypper >/dev/null 2>&1; then run_install 'zypper --non-interactive install zellij' && exit 0; fi
if command -v apk >/dev/null 2>&1; then run_install 'apk add zellij' && exit 0; fi
if ! command -v zellij >/dev/null 2>&1; then
  echo "zellij 설치에 실패했습니다." >&2
  exit 1
fi
