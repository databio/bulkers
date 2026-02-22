# bulkers

Multi-container environment manager.

Port from python to rust. 

Alpha testing.

## Install

```bash
curl -sL https://raw.githubusercontent.com/databio/bulkers/master/install.sh | bash
```

This downloads the binary and adds a shell function to your `~/.bashrc` (or `~/.zshrc`) that enables `bulkers activate` to modify your current shell and `bulkers deactivate` to restore it.

### Manual install

1. Download binary:

```bash
# Linux x86_64
curl -sL https://github.com/databio/bulkers/releases/latest/download/bulkers-Linux-musl-x86_64.tar.gz | tar xz && mv bulkers ~/.local/bin/

# macOS Apple Silicon
curl -sL https://github.com/databio/bulkers/releases/latest/download/bulkers-macOS-arm64.tar.gz | tar xz && mv bulkers ~/.local/bin/

# macOS Intel
curl -sL https://github.com/databio/bulkers/releases/latest/download/bulkers-macOS-x86_64.tar.gz | tar xz && mv bulkers ~/.local/bin/
```

2. Add to `~/.bashrc` (or `~/.zshrc`):

```bash
# >>> bulkers initialize >>>
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
# <<< bulkers initialize <<<
```

Or build from source: `cargo install --path .`
