//go:build gomlx

// Opt-in support for hiveGo's neural players ("fnn" for alpha-beta, "a0fnn"
// for MCTS), which are the ones its README claims beat commercial Hive apps.
//
// Kept behind a tag because importing them pulls in GoMLX and the XLA runtime,
// which is a large dependency that the linear-scorer match does not need.
// Build with: go build -tags gomlx ./cmd/hivebridge
package main

import (
	_ "github.com/gomlx/gomlx/backends/default"
	_ "github.com/janpfeifer/hiveGo/internal/ai/gomlx"
)
