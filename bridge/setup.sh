#!/usr/bin/env bash
# Prepare a hiveGo checkout that can play against hive-engine.
#
# Clones janpfeifer/hiveGo, drops its local-path replace directive, installs
# cmd/hivebridge, and builds both engines. Re-runnable.
#
#   bridge/setup.sh [checkout-dir]     default: /tmp/hiveGo
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIR="${1:-/tmp/hiveGo}"

# hiveGo needs Go >= 1.26. Override with GO=/path/to/go if it is not on PATH.
GO="${GO:-go}"
if ! command -v "$GO" >/dev/null 2>&1; then
  echo "setup: no Go toolchain found. Install Go 1.26+ and re-run, or set GO=/path/to/go" >&2
  echo "  curl -fsSL https://go.dev/dl/go1.26.5.linux-amd64.tar.gz | tar -C /tmp/goroot --strip-components=1 -xz" >&2
  exit 1
fi

if [[ ! -d "$DIR/.git" ]]; then
  git clone --depth 1 https://github.com/janpfeifer/hiveGo.git "$DIR"
fi

# hiveGo's go.mod carries `replace github.com/gomlx/gomlx => ../gomlx/gomlx`,
# an author-local path that does not exist anywhere else. We never build the
# GoMLX-backed players, so dropping the line is enough; keeping it makes even
# `go list` fail.
if grep -q '^replace github.com/gomlx/gomlx' "$DIR/go.mod"; then
  grep -v '^replace github.com/gomlx/gomlx' "$DIR/go.mod" > "$DIR/go.mod.tmp"
  mv "$DIR/go.mod.tmp" "$DIR/go.mod"
  echo "setup: removed the gomlx replace directive from go.mod"
fi

# hiveGo omits Hive's height gate for beetles: internal/state/pieces.go lets a
# beetle climb onto any occupied neighbour, and a stacked beetle move to all
# six, with no check that it can clear the two hexes flanking the step. Our
# differential fuzz finds a position where the two engines disagree within ~50
# random playouts. Without this patch the engines are playing different games
# and no match result means anything, so apply it or stop.
if git -C "$DIR" diff --quiet -- internal/state/pieces.go; then
  if git -C "$DIR" apply --check "$ROOT/bridge/hivego-beetle-gate.patch" 2>/dev/null; then
    git -C "$DIR" apply "$ROOT/bridge/hivego-beetle-gate.patch"
    echo "setup: applied the beetle-gate rule fix to hiveGo"
  else
    echo "setup: ERROR - bridge/hivego-beetle-gate.patch no longer applies." >&2
    echo "  hiveGo's beetleMoves has changed upstream. Check whether the gate" >&2
    echo "  rule is now implemented there; if not, refresh the patch." >&2
    exit 1
  fi
else
  echo "setup: hiveGo's pieces.go is already modified, leaving it alone"
fi

mkdir -p "$DIR/cmd/hivebridge"
cp "$ROOT/bridge/hivebridge.go" "$DIR/cmd/hivebridge/main.go"
cp "$ROOT/bridge/hivebridge_gomlx.go" "$DIR/cmd/hivebridge/gomlx.go"

echo "setup: building the Rust bridge"
cargo build --release -p hive-engine --example bridge --manifest-path "$ROOT/Cargo.toml"

echo "setup: building the Go bridge"
( cd "$DIR" && GOFLAGS=-mod=mod "$GO" build -o "$DIR/hivebridge" ./cmd/hivebridge )

# Build the neural-player binary from the same tree in the same step. Keeping
# these two in lockstep matters: a hivebridge-gomlx left over from before the
# beetle-gate patch silently plays by the old rules and aborts mid-match.
if [[ "${SKIP_GOMLX:-0}" != "1" ]]; then
  echo "setup: building the Go bridge with hiveGo's neural players (SKIP_GOMLX=1 to skip)"
  ( cd "$DIR" && GOFLAGS=-mod=mod "$GO" build -tags gomlx -o "$DIR/hivebridge-gomlx" ./cmd/hivebridge )
fi

cat <<EOF

Ready. Run a match with:

  $DIR/hivebridge \\
    -rust $ROOT/target/release/examples/bridge \\
    -games 20 -think 100ms -ai 'linear,ab,max_time=100ms'
EOF
