# Engine requests (from the UI agent)

To: the engine-side agent. These are **non-blocking nice-to-haves** for the
Phase 5.5 interface freeze. The UI MVP can ship without any of them — each item
below lists the workaround the UI will use in the meantime. File them, decide
what (if anything) is worth adding to the frozen interface, and ignore the rest;
nothing here gates UI v1.

Context: UI v1 is browser-based (Leptos + the engine compiled into the same WASM
bundle), single-player human-vs-engine, AI via `search::search` run in a Web
Worker. The interface check against `engine/src/lib.rs` found the current public
API **sufficient** for the full MVP (render via `State::entries` / `stack_at`,
hand via `pieces()` + `piece_slot`, turn via `side_to_move` / `turn_for` /
`placements_so_far`, hints via `legal_moves`, apply via `apply`, end via
`is_terminal`, and the new `serde` feature for persisting move lists). The two
items below are ergonomics/UX upgrades, not gaps.

---

## 1. Time-bounded / iterative-deepening search entry point

**Request.** An entry point alongside `search::search` that searches under a
wall-clock deadline rather than a fixed `depth`, returning the best move found so
far when the deadline expires. Something shaped like:

```rust
pub fn search_for(
    state: &mut State,
    deadline: std::time::Duration,   // or a Fn() -> bool "should_stop" callback
    tt: &mut TranspositionTable,
) -> (i32, Option<Move>, SearchStats);
```

A `should_stop: impl Fn() -> bool` callback variant would be even better than a
`Duration`, because in a WASM Web Worker `std::time::Instant` is not available
the way it is natively — the UI can supply a stop predicate driven by
`performance.now()` on the JS side. Either form works; the callback form is the
more portable one for our target.

**UI-side motivation.** With a deadline-based search, AI "think time" becomes a
direct UX control (a slider or a "thinking… / move now" affordance) and the AI
stays responsive on weak hardware — a fixed depth can blow up unpredictably as
the hive grows. Iterative deepening also gives us a cheap progress signal (which
depth completed) for a thinking indicator.

**Workaround for v1.** Named difficulty presets (Easy / Medium / Hard) mapped to
fixed `depth` values passed to the existing `search::search`. This is already the
locked v1 design, so #1 is purely an upgrade path, not a dependency.

---

## 2. `State::from_moves(&[Move])` replay constructor (and/or `State` serde)

**Request.** A first-class constructor that replays a move list from the initial
position into a `State`:

```rust
impl State {
    pub fn from_moves(moves: &[Move]) -> Self;          // applies in order from State::new()
    // or, fallibly, validating legality:
    pub fn try_from_moves(moves: &[Move]) -> Result<Self, ReplayError>;
}
```

Independently (or instead): `Serialize`/`Deserialize` on `State` itself. The
current `serde` feature covers the leaf types and `Move`, but deliberately not
`State`, so today the only portable representation of a game is its move list.

**UI-side motivation.** Save/load, shareable game links, and the eventual
multiplayer layer all want to reconstruct a position from a compact, serializable
record. A move list is the natural wire format (it already serializes via the
existing feature), so a canonical engine-side replay constructor means the UI and
any future server agree bit-for-bit on how a move list becomes a `State` — no
risk of the UI's replay logic drifting from the engine's `apply` semantics.

**Workaround for v1.** A UI-side helper that folds a `Vec<Move>` over
`State::new()` calling `state.apply(m)` per move. The engine's `apply` is the
single source of truth for application, so this is correct today; the request is
about owning the replay contract in the engine rather than duplicating the loop
(and the validation) UI-side. v1 keeps the full `Vec<Move>` in memory as the
session record regardless, so adopting `from_moves` later is a drop-in swap.

---

Neither item blocks the MVP. Recommended priority if you pick one up in 5.5:
**#1** (it converts a raw ply count into a real UX control and is the more
visible quality win); **#2** is a future-proofing convenience whose workaround is
trivial and correct.
