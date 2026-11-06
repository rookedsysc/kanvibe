if command -v herdr >/dev/null 2>&1 || test -x "$HOME/.local/bin/herdr"; then exit 0; fi
kanvibe_herdr_installer=$(mktemp) || exit 1
trap 'rm -f "$kanvibe_herdr_installer"' EXIT
curl -fsSL https://herdr.dev/install.sh -o "$kanvibe_herdr_installer" || exit 1
HERDR_INSTALL_DIR="$HOME/.local/bin" sh "$kanvibe_herdr_installer" || exit 1
command -v herdr >/dev/null 2>&1 || test -x "$HOME/.local/bin/herdr"
