#!/usr/bin/env bash
set -euo pipefail

REPO="databio/bulkers"
INSTALL_DIR="$HOME/.local/bin"

# Determine script's directory (empty if piped from curl)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}" 2>/dev/null)" 2>/dev/null && pwd || echo "")"

mkdir -p "$INSTALL_DIR"

if [ -n "$SCRIPT_DIR" ] && grep -q '^name = "bulker"' "$SCRIPT_DIR/Cargo.toml" 2>/dev/null; then
  # Local mode: build from source
  echo "Building from source..."
  cargo build --release --manifest-path "$SCRIPT_DIR/Cargo.toml"
  if ! cp "$SCRIPT_DIR/target/release/bulker" "$INSTALL_DIR/bulker" 2>/dev/null; then
    if [ "${1:-}" = "--force" ]; then
      rm -f "$INSTALL_DIR/bulker"
      cp "$SCRIPT_DIR/target/release/bulker" "$INSTALL_DIR/bulker"
    else
      echo "Error: bulker binary is in use (a container may be running)."
      echo "Stop running containers, or re-run with: ./install.sh --force"
      exit 1
    fi
  fi
  chmod +x "$INSTALL_DIR/bulker"
else
  # Remote mode: download from GitHub releases
  OS="$(uname -s)"
  ARCH="$(uname -m)"

  case "$OS" in
    Linux)
      case "$ARCH" in
        x86_64)  ASSET="bulker-Linux-musl-x86_64.tar.gz" ;;
        aarch64) ASSET="bulker-Linux-musl-arm64.tar.gz" ;;
        *)       echo "Unsupported architecture: $ARCH"; exit 1 ;;
      esac
      ;;
    Darwin)
      case "$ARCH" in
        x86_64)  ASSET="bulker-macOS-x86_64.tar.gz" ;;
        arm64)   ASSET="bulker-macOS-arm64.tar.gz" ;;
        *)       echo "Unsupported architecture: $ARCH"; exit 1 ;;
      esac
      ;;
    *)
      echo "Unsupported OS: $OS"
      exit 1
      ;;
  esac

  echo "Downloading $ASSET..."
  curl -sL "https://github.com/$REPO/releases/latest/download/$ASSET" | tar xz -C "$INSTALL_DIR"
  chmod +x "$INSTALL_DIR/bulker"
fi

VERSION="$("$INSTALL_DIR/bulker" --version 2>/dev/null || echo "bulker (unknown version)")"
echo "Installed $VERSION to $INSTALL_DIR/bulker"

# Detect shell rc file
SHELL_NAME="$(basename "$SHELL")"
case "$SHELL_NAME" in
  zsh)  RC_FILE="$HOME/.zshrc" ;;
  bash) RC_FILE="$HOME/.bashrc" ;;
  *)    RC_FILE="$HOME/.bashrc" ;;
esac

MARKER="# >>> bulker initialize >>>"
END_MARKER="# <<< bulker initialize <<<"

# Get shell function from bulker itself
SHELL_FUNC="$("$INSTALL_DIR/bulker" init-shell "$SHELL_NAME")"

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
