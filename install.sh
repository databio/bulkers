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

# Download and install binary
echo "Downloading $ASSET..."
mkdir -p "$INSTALL_DIR"
curl -sL "https://github.com/$REPO/releases/latest/download/$ASSET" | tar xz -C "$INSTALL_DIR"
chmod +x "$INSTALL_DIR/bulkers"
echo "Installed bulkers to $INSTALL_DIR/bulkers"

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
      eval "$(\command bulkers activate -e "$@")"
      ;;
    deactivate)
      if [ -n "$BULKER_ORIG_PATH" ]; then
        export PATH="$BULKER_ORIG_PATH"
        unset BULKERCRATE BULKERPATH BULKERPROMPT BULKERSHELLRC BULKER_ORIG_PATH
      fi
      ;;
    *)
      \command bulkers "$@"
      ;;
  esac
}
# <<< bulkers initialize <<<'

# Append shell function if not already present
if [ -f "$RC_FILE" ] && grep -qF "$MARKER" "$RC_FILE"; then
  echo "Shell function already present in $RC_FILE"
else
  echo "" >> "$RC_FILE"
  echo "$SHELL_FUNC" >> "$RC_FILE"
  echo "Added shell function to $RC_FILE"
fi

echo ""
echo "Done! Restart your shell or run:"
echo "  source $RC_FILE"
