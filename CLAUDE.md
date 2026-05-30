# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

A Cargo workspace containing the Hive board-game engine and (planned) a playable UI. The workspace exists so engine work and UI work can proceed in parallel under clear ownership without breaking each other.

```
Hivemind-rust/                  # workspace root (you are here)
  Cargo.toml                    # [workspace] manifest, lists members
  Cargo.lock                    # shared lockfile
  engine/                       # the engine crate — see engine/CLAUDE.md
    Cargo.toml
    src/ tests/ examples/
  ui/                           # planned; added by the hive-ui agent
  .github/workflows/test.yml    # CI for the whole workspace
```

## Ownership

- **`engine/`** is owned by the default engine agent in this session. UI agent never edits files here.
- **`ui/`** is owned by the `hive-ui` subagent (defined at `~/.claude/agents/hive-ui.md`). Engine agent never edits files here.
- **Root-level files** — workspace `Cargo.toml`, `Cargo.lock`, `.gitignore`, root `CLAUDE.md`, `.github/workflows/` — are shared. Whichever side touches one of them calls it out in the PR description so the other side knows.
- Adding `ui/` as a workspace member is the canonical first shared-files PR; the UI agent owns that PR.

## Commands

These work from this workspace root:

```bash
cargo build --release                   # builds every workspace member
cargo test --release                    # tests every workspace member
cargo test                              # debug; runs proptest with more invariants enabled
cargo test --release -p hive-engine     # engine only
cargo run --release --example perft_bench [depth]
cargo run --release --example search_bench [depth] [tt_log2]
```

For engine-specific architecture, invariants, and performance choices, see `engine/CLAUDE.md`.

## Workflow

- Branch-per-change + PR via `gh pr create`. CI must be green before merge. No direct pushes to `main`.
- Suggested branch prefixes: `perf/`, `feat/`, `fix/`, `refactor/`, `ci/`, `docs/` for engine work; the UI agent uses the same prefixes scoped under `ui/` (e.g. `ui/feat/board-render`).
- Commit message style: specific imperative, end with `Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>`.

## Phase status

The engine is at end of Phase 5 (correctness + Zobrist + make-unmake + incremental caches + alpha-beta search + perf pass — perft depth 5 at ~2.26M nodes/sec, ~3.36× the reference TypeScript implementation). **Phase 5.5 (interface freeze for UI parallelization)** is next; this workspace migration is the prep step for it.

Full phase plan and project memory: `/Users/gregor30/.claude/projects/-Users-gregor30-Dev-ClaudeCodeHive/memory/`.
