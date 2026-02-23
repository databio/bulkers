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

# Check for local build (only works when script is run as a file, not piped)
if [ -n "${BASH_SOURCE[0]+x}" ]; then
  SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
  LOCAL_BUILD="$SCRIPT_DIR/target/release/bulkers"
else
  LOCAL_BUILD=""
fi

if [ -n "$LOCAL_BUILD" ] && [ -f "$LOCAL_BUILD" ]; then
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
END_MARKER="# <<< bulkers initialize <<<"

# Get shell function from bulkers itself
SHELL_FUNC="$("$INSTALL_DIR/bulkers" init-shell "$SHELL_NAME")"

# Install shell function (replace existing or append)
if [ -f "$RC_FILE" ] && grep -qF "$MARKER" "$RC_FILE"; then
  # Replace existing block
  tmpfile=$(mktemp)
  awk -v start="$MARKER" -v end="$END_MARKER" '
    $0 == start { skip=1; next }
    $0 == end { skip=0; next }
    !skip { print }
  ' "$RC_FILE" > "$tmpfile"
  printf '%s\n' "$SHELL_FUNC" >> "$tmpfile"
  mv "$tmpfile" "$RC_FILE"
  echo "Updated shell function in $RC_FILE"
else
  echo "" >> "$RC_FILE"
  printf '%s\n' "$SHELL_FUNC" >> "$RC_FILE"
  echo "Added shell function to $RC_FILE"
fi

echo ""
echo "Done! Restart your shell or run:"
echo "  source $RC_FILE"
