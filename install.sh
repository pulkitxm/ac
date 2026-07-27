#!/usr/bin/env bash
# install.sh - put `ac` on PATH and wire up shell completion.
set -euo pipefail

AC_HOME="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN_DIR="${AC_BIN_DIR:-$HOME/.local/bin}"

mkdir -p "$BIN_DIR"
ln -sf "$AC_HOME/bin/ac" "$BIN_DIR/ac"
chmod +x "$AC_HOME/bin/ac"

echo "linked $BIN_DIR/ac -> $AC_HOME/bin/ac"
echo
echo "Add to your ~/.zshrc if not already present:"
echo
echo "  export PATH=\"$BIN_DIR:\$PATH\""
echo "  export AC_HOME=\"$AC_HOME\""
echo "  fpath=(\"$AC_HOME/completions\" \$fpath)"
echo "  autoload -Uz compinit && compinit"
echo
echo "For bash, instead source the completion directly:"
echo
echo "  source \"$AC_HOME/completions/ac.bash\""
