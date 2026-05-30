# CLAUDE.md (engine crate)

This file provides guidance to Claude Code when working inside the `engine/` crate of the Hivemind-rust workspace. The root-level CLAUDE.md covers workspace-wide concerns; this file is engine-specific.

Path references in this file are **crate-relative** (e.g. `src/state.rs` means `engine/src/state.rs`).

## What this is

A Rust engine for the board game **Hive (base game only)**. The build proceeds in strict phases; each phase ends with a passing test suite before the next begins. Phases 1–4 (correctness, Zobrist hashing, make-unmake, incremental caches) are done; Phase 5 (profile-driven optimization) has shipped both the brief's candidates and a follow-up perf pass. Phase 5.5 (interface freeze for UI parallelization) is next.

## Commands

These work from the workspace root **or** from `engine/`. From the workspace root, `cargo` auto-selects this crate when invoking examples and most other commands; `-p hive-engine` makes it explicit.

```bash
# Build
cargo build --release

# All tests (release is fast; debug runs proptest with more invariants enabled)
cargo test --release
cargo test                                       # debug — slower but exercises more checks
cargo test --release -p hive-engine              # only this crate (useful once ui/ exists)

# Single test by name (substring match across all test binaries)
cargo test --release round_trip_beetle_climb_and_descend
cargo test --release --lib search                # only lib unit tests matching 'search'

# Benchmarks (release only — debug numbers are meaningless)
cargo run --release --example perft_bench [depth]                       # default depth=3
cargo run --release --example search_bench [depth] [tt_log2]            # defaults 4, 18
```

`perft_bench` is the canonical throughput baseline — every perf change in `git log` cites a depth-5 nodes/sec number from it. `search_bench` exercises alpha-beta + TT from the initial position; node counts come from the search policy, throughput from the engine.

## Architecture

**Hybrid state representation.** Source of truth is `pieces: [PieceSlot; 22]` in `src/state.rs` — fixed-size, stack-allocated. Coord-keyed lookup is via `Board` in `src/board.rs`, which is fully reconstructible from `pieces` (see `State::board_from_pieces`, used by tests). Mutation funnels through `State::apply` — `Board`'s mutators are `pub(crate)` so only `state::apply` can write.

**Make-unmake from Phase 3.** Each `apply` pushes an `UndoRecord` onto `state.undo_stack`; `unapply` pops and reverses, including the Phase 4 cache deltas (`perimeter`, `placement_legality_white`, `placement_legality_black`). The round-trip proptest in `tests/proptest_invariants.rs` runs `apply` followed by `unapply` on every step and asserts bit-for-bit equality on the whole `State` — **this is the safety net for any change to `apply`/`unapply`, including cache updates and the undo emplacement**. Don't disable it.

**Move encoding.** Single `Move` enum: `Place { piece, to } | Slide { piece, to } | Pass`. `Slide` covers slide/hop/climb uniformly — legality differs by piece type (resolved in `gen/`) but application is uniform. `Pass` is a real `Move` variant, emitted only when no other move exists. Move generation entry point is `State::legal_moves()`.

**Coordinate convention.** Axial `(q, r)` with neighbour deltas `(+1,0), (-1,0), (0,+1), (0,-1), (+1,-1), (-1,+1)`. Defined in exactly one place: `src/coord.rs`. Every direction-dependent piece of logic depends on this — do not introduce a second convention.

**PieceId layout.** Each physical piece is distinct (the two beetles are NOT interchangeable in the engine — this keeps a future RL action-index stable). Indices `0..11` are white, `11..22` are black, both in canonical order `[Q, B, B, G, G, G, S, S, A, A, A]`. The Zobrist hash, in contrast, *is* keyed on `piece_type` not `PieceId`, so interchangeable pieces collide deliberately in the TT.

**Per-player turn counters.** `State { white_turn, black_turn, side_to_move, ... }`. The "queen must be placed by 4th turn" rule reads off the active player's counter, not a combined ply count.

## Critical invariants (proptest-guarded)

The proptest in `tests/proptest_invariants.rs` runs every step of random legal play and asserts:
- **Board ↔ pieces coherence**: `state.board()` equals what `board_from_pieces()` rebuilds.
- **One Hive**: occupied coords form a single 6-connected component (or are empty).
- **Round-trip**: `apply(m); unapply()` restores the full `State` bit-for-bit.
- **Zobrist from-scratch**: `state.zobrist() == zobrist::from_scratch(&state)` after every step.
- **Cache from-scratch**: `state.perimeter()` and `state.placement_legality(color)` equal their from-scratch builds (sorted slices).

If you break any of these, the proptest catches it — usually within a few cases.

## Performance — settled choices

Don't relitigate these without a profile-backed reason. If you have one, flag it rather than silently diverging.

- **`Board.cells` is a flat 64×64 array indexed by `pack(c) = (q+32)*64 + (r+32)`**, with a parallel sorted `SmallVec<[Coord; 22]>` for iteration and `PartialEq`. `is_occupied` / `top_at` are a single memory access. 8 KB inline in `State`; cloning is one memcpy but search uses make-unmake and doesn't pay this.
- **`State` cache sets** (`perimeter`, `placement_legality_*`) are sorted `SmallVec<[Coord; 32]>` (`CoordSet`), maintained via `cs_insert` / `cs_remove`. Public API returns `&[Coord]`.
- **`UndoRecord` is emplaced** into `undo_stack` via `Vec::spare_capacity_mut()[0].write(...)` + `set_len` to skip a stack→heap memcpy per apply.
- **Tarjan articulation** (`rules::articulation_points`) uses stack arrays `[_; 22]` for all working storage; coord→index is binary search over the sorted `Board.occupied_coords()`.
- **Ant / spider DFS** use stack `SmallVec` for visited and destinations — no `HashSet` allocations per move-gen call.

The hot paths are `state.apply()` / `state.unapply()` and `state.legal_moves()` calling into `src/gen/`. Pre-existing `HashSet`/`HashMap` usage in those paths is intentional only where allocations don't repeat (e.g. tests, from-scratch builders).

## Phase ordering — work style

**Correct first, optimized second.** Each phase ends with the test suite green before the next begins. *Do not* build later-phase features early. When implementation reveals a conflict with a phase decision, **stop and raise it** rather than silently diverging.

**Optimization decisions in Phase 5+ must be justified by a profile**, not a hunch. The session that produced the perf-hashset-tarjan branch staged five changes with measurements between each — the surprising results (HashSet caches were only +5%, but `affected_coords_for`'s per-apply HashSet alloc was +32%, and flat-array Board was +95%) are the reason this rule exists.

## Explicit non-goals (engine-side)

Do not build any of these inside this crate without explicit user direction:
- Hive expansions (Mosquito / Ladybug / Pillbug — reserved enum variants only in `PieceType`)
- Symmetry canonicalization in the hash key
- UHP protocol, network play
- The UI itself (lives in the sibling `ui/` crate, built by the `hive-ui` subagent)
- The AlphaZero pipeline (planned Phase 8, separate concern)

The plan file at `/Users/gregor30/.claude/plans/hive-engine-project-eventual-charm.md` references the user's original brief.
