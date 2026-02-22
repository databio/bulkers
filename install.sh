#!/usr/bin/env bash
set -euo pipefail

REPO="databio/bulkers"
INSTALL_DIR="$HOME/.local/bin"

# Detect OS and architecture
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Linux)
    case "$ARCH" in
      x86_64)  ASSET="bulkers-Linux-musl-x86_64.tar.gz" ;;
      aarch64) ASSET="bulkers-Linux-musl-aarch64.tar.gz" ;;
      *)       echo "Unsupported architecture: $ARCH"; exit 1 ;;
    esac
    ;;
  Darwin)
    case "$ARCH" in
      x86_64)  ASSET="bulkers-macOS-x86_64.tar.gz" ;;
      arm64)   ASSET="bulkers-macOS-arm64.tar.gz" ;;
      *)       echo "Unsupported architecture: $ARCH"; exit 1 ;;
    esac
    ;;
  *)
    echo "Unsupported OS: $OS"
    exit 1
    ;;
esac

# Install binary
mkdir -p "$INSTALL_DIR"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LOCAL_BUILD="$SCRIPT_DIR/target/release/bulkers"

if [ -f "$LOCAL_BUILD" ]; then
  cp "$LOCAL_BUILD" "$INSTALL_DIR/bulkers"
  chmod +x "$INSTALL_DIR/bulkers"
  echo "Installed bulkers from local build to $INSTALL_DIR/bulkers"
else
  echo "Downloading $ASSET..."
  curl -sL "https://github.com/$REPO/releases/latest/download/$ASSET" | tar xz -C "$INSTALL_DIR"
  chmod +x "$INSTALL_DIR/bulkers"
  echo "Installed bulkers to $INSTALL_DIR/bulkers"
fi

# Detect shell rc file
SHELL_NAME="$(basename "$SHELL")"
case "$SHELL_NAME" in
  zsh)  RC_FILE="$HOME/.zshrc" ;;
  bash) RC_FILE="$HOME/.bashrc" ;;
  *)    RC_FILE="$HOME/.bashrc" ;;
esac

MARKER="# >>> bulkers initialize >>>"

SHELL_FUNC='# >>> bulkers initialize >>>
bulkers() {
  case "$1" in
    activate)
      shift
      _BULKER_OLD_PS1="$PS1"
      eval "$(\command bulkers activate -e "$@")"
      if [ -n "$BULKERCRATE" ]; then
        PS1="(\[\033[01;93m\]${BULKERCRATE}\[\033[00m\]) ${_BULKER_OLD_PS1}"
      fi
      ;;
    deactivate)
      if [ -n "$BULKER_ORIG_PATH" ]; then
        export PATH="$BULKER_ORIG_PATH"
        if [ -n "$_BULKER_OLD_PS1" ]; then
          PS1="$_BULKER_OLD_PS1"
        fi
        unset BULKERCRATE BULKERPATH BULKERPROMPT BULKERSHELLRC BULKER_ORIG_PATH _BULKER_OLD_PS1
      fi
      ;;
    *)
      \command bulkers "$@"
      ;;
  esac
}
# <<< bulkers initialize <<<'

# Install shell function (replace existing or append)
END_MARKER="# <<< bulkers initialize <<<"
if [ -f "$RC_FILE" ] && grep -qF "$MARKER" "$RC_FILE"; then
  # Replace existing block
  tmpfile=$(mktemp)
  awk -v start="$MARKER" -v end="$END_MARKER" -v func="$SHELL_FUNC" '
    $0 == start { skip=1; printed=1; print func; next }
    $0 == end { skip=0; next }
    !skip { print }
  ' "$RC_FILE" > "$tmpfile"
  mv "$tmpfile" "$RC_FILE"
  echo "Updated shell function in $RC_FILE"
else
  echo "" >> "$RC_FILE"
  echo "$SHELL_FUNC" >> "$RC_FILE"
  echo "Added shell function to $RC_FILE"
fi

echo ""
echo "Done! Restart your shell or run:"
echo "  source $RC_FILE"
