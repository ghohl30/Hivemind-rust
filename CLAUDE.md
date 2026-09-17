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

Phases 1–5 are done (correctness + Zobrist + make-unmake + incremental caches + alpha-beta search + perf pass — perft depth 5 at ~2.26M nodes/sec, ~3.36× the reference TypeScript implementation). **Phase 5.5 (interface freeze for UI parallelization) shipped in PR #8** and has held: every engine change since has been purely additive, and the UI has never been broken by one.

**Phase 6 (playing strength) is in progress.** Landed so far:

- PR #18 — `search_bounded`, a time-bounded search entry point. Deliberately clock-free: the caller supplies a `should_stop` predicate, because `std::time::Instant` compiles on `wasm32-unknown-unknown` and then panics at runtime, and this crate is compiled into the UI's WASM bundle.
- PR #19 — `Eval` trait (static dispatch) plus `examples/gauntlet.rs`, a self-play harness with randomised openings and colour-swapped pairs.
- PRs #20–#23 — evaluation and move ordering: convex owner-agnostic queen-surround, beetle-on-queen, killer moves. Together **61.1% ± 3.7% over 180 games** against the previous evaluation, and depth 6 from 105,879 nodes / 84.6ms to 44,584 / 37.3ms.

**First external benchmark (2026-09-17).** Until now every strength number was self-play against an older version of ourselves, which measures progress but not level. Against [janpfeifer/hiveGo](https://github.com/janpfeifer/hiveGo) at its own default 3 s/move control, 100 games per opponent with colours swapped:

| hiveGo opponent | score | Elo |
|---|---|---|
| `linear,ab` — hand-tuned linear eval | 71.0% ± 4.0% | +155 |
| `fnn=#0,ab` — pretrained neural eval | 78.5% ± 3.7% | +225 |
| `a0fnn=#0,mcts` — AlphaZero net + MCTS | 67.5% ± 4.1% | +127 |

We are stronger than every AI that engine ships. Two results matter more than the scoreline:

- **We convert time into strength poorly.** 30× the budget (100 ms → 3 s) bought ~1.6 extra plies — depth 3.3–4.0 → 5.2–5.6 — on 30× the nodes (~21k → ~650k per move). Every margin above fell 7.5–15.5 points versus the same matches at 100 ms, *including* against their cheapest evaluator, so this is not the cost of neural evaluation on their side. It is Hive's branching factor, and it means an opponent who searches better gains more from a long control than we do.
- **Our move generator is validated against an independent implementation.** 78k+ plies of random playout agree exactly, after the cross-check found a genuine rules bug — in hiveGo, not in us (their beetle never applies freedom-to-move at height). This is stronger correctness evidence than perft against ourselves.

**Evaluation changes are gated on gauntlet win rate, not node counts** — Hive has no captures, so material is constant and essentially all strength lives in the evaluation. A change can cut nodes and play worse. Budget ~180 games (two seeds pooled, 100ms/move) for roughly ±3.7% at 1 s.e.; smaller runs mislead (an 80-game run read 55% where 300 games put the same change at 48.5%).

Open engine follow-ups, in rough value order — **reordered by the external benchmark, which moved search efficiency above evaluation work**:

1. **Search efficiency — now the highest-value area.** 1.6 plies per 30× time is the measured symptom; the cause is that we widen rather than deepen. Concretely: the `killer_moves` slot-ordering bug in `search.rs` (killers are pushed in `moves` order, so killer[1] can precede the more recent killer[0]), then late-move reductions and aspiration windows. This is where strength against a *searching* opponent lives, and it is not visible in self-play at a fixed control, because both sides scale equally badly.
2. **Repetition detection.** Now has a price tag: 12 of 52 draws in the external series, and every opponent there can see repetitions while our search cannot — so we walk into loops we are unable to evaluate. Also still dilutes gauntlet numbers via the 300-ply cap.
3. Delete the now-beaten `LegacyEval` scaffold.
4. `examples/match.rs` — its "10 games" are one deterministic game tallied ten times.

**Evaluation tuning is no longer the obvious next lever.** It was, when the only yardstick was our own previous evaluation; the external result says the search is the weaker half.

Full phase plan and project memory: `~/.claude/projects/-home-gregor-Repos-Hivemind-rust/memory/`.

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
