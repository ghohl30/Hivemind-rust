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

## Remote GPU compute (for the planned RL phase)

The engine is pure Rust, but an RL agent will eventually be trained on the game — that
means self-play data generation, network training, and seed/hyperparameter sweeps, all of
which want a GPU. Remote compute for this is already solved and proven; the reference
implementation is `~/Documents/testCloudCompute` (see its `HANDOFF.md`). **Don't rebuild
it — port it.** That path is machine-local, so vendor `cloudrun/` into this repo when the
RL work starts rather than depending on it in place.

One thing the spike did not cover and this project will hit immediately: the Rust/Python
boundary. Modal tasks are Python, and self-play needs the Rust engine running inside the
remote container. Decide early whether that means a PyO3 binding, a static binary invoked
as a subprocess, or moving rollouts off the GPU box entirely — whichever it is, the engine
build has to be pinned in the Modal image like any other dependency.

**Provider is Modal**, chosen for budget safety rather than price (~1.5–2× RunPod rates).
There is no instance to leak: the container exits and billing stops, so a crashed
orchestrator can't leave a GPU running overnight. Per-second billing, no reaper scripts.

**The architectural rule worth keeping:** task code never imports `modal`.

```
tasks/<task>/core.py     def run(cfg: dict) -> dict   # pure, no modal import
tasks/modal_app.py       @app.function(...) wrapper   # the ONLY modal-aware file
```

This gives free local debugging (`--local` runs the identical code path on CPU — make
that the default habit; never burn GPU seconds on a syntax error), plain-function
testability, and a one-file port if the provider changes.

**Budget enforcement** is three layers, and the load-bearing one is the middle:
1. Modal workspace spend limit (server-side, monthly) — catastrophe backstop only.
2. `--max-cost` in CHF converted to a hard Modal timeout, `retries=0`. A runaway loop
   is killed by the clock before it outspends its declaration.
3. Local daily ledger (`runs/ledger.jsonl`) refuses a run before any remote work if
   `spent_today + max_cost > DAILY_BUDGET_CHF`. It charges the declared ceiling up
   front, so the check stays correct if the process dies mid-run. Estimates are
   client-side wall clock against a local rate table (Modal's billing API is
   Team/Enterprise only); measured ~2× high, i.e. safe direction.

Exit codes for branching: `0` ok, `1` run failed or hit its ceiling, `2` refused before
spending anything.

**Costs are small.** T4 is ~0.00016 USD/sec: MNIST to 98.4% accuracy cost 0.008 CHF and
under a minute wall clock. A 1 CHF/day budget is ~120 such runs. The real-work budget is
10–20 CHF/day. Re-verify the rate table if it's stale — `cloudrun/pricing.py` carries
`LAST_VERIFIED` and warns after 60 days.

**Reproducibility is achievable and verified** — two runs on separate T4s produced
bit-identical checkpoints. Requires all of: deterministic algorithms + `cudnn.benchmark=False`,
all RNGs seeded, `CUBLAS_WORKSPACE_CONFIG=:4096:8` set in the *image* (cuBLAS reads it at
CUDA init, before your code runs), `num_workers=0` with a seeded generator, exactly one
pinned GPU type (fallback lists like `gpu=["H100","A100"]` change numerics — let the job
queue instead), and every dependency version pinned.

**Gotchas that already cost time:**
- `volume.reload()` only works inside a running function; client-side it throws.
- The remote function must `volume.commit()` before returning or artifacts are invisible.
- Decorator values are static — use `Function.with_options(gpu=…, timeout=…, retries=…)`
  for dynamic config. Options cannot be unset (a decorator-level GPU can't be dropped).
- Default timeout is 300s (max 24h). Always set it explicitly.
- Make artifact download best-effort: a checkpoint you can't fetch must not discard
  metrics you already paid for.

**Known gaps:** the timeout kill has never actually been observed firing (arithmetic is
tested, ~2 rappen to confirm live); manifests record `git_sha: null` because git was
missing on the machine that ran the spike — fix that before trusting a manifest to link a
result to its code; concurrent launches can each pass the ledger preflight before any
writes its entry, so fix the ledger before running parallel sweeps.

```bash
cloudrun doctor --json                            # auth, budget, price freshness; free
cloudrun estimate <task> --gpu T4 --max-cost 0.10
cloudrun run <task> --local --set epochs=1        # free CPU debugging
cloudrun run <task> --gpu T4 --max-cost 0.10 --set seed=0 --json
cloudrun ledger --today
```

`--set key=value` parses values as JSON (`--set lr=0.001`, `--set synthetic=true`).
