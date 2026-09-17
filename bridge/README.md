# bridge — playing hive-engine against janpfeifer/hiveGo

An external-opponent harness. Every strength number this project has produced
so far is self-play against an older version of itself, which measures progress
but cannot measure *level*. [hiveGo](https://github.com/janpfeifer/hiveGo) is an
independent Hive engine with a linear evaluator, a neural board evaluator, and
an AlphaZero-style MCTS player, so it gives us an outside yardstick.

## How it fits together

hiveGo has no UHP or any other external-engine protocol, so there is nothing to
speak to. Instead:

- `engine/examples/bridge.rs` turns hive-engine into a line-protocol server on
  stdin/stdout.
- `bridge/hivebridge.go` is a `main` package that gets copied *into* a hiveGo
  checkout (its game code lives under `internal/`, so it cannot be imported
  from anywhere else). It owns the game loop, runs hiveGo's AI in-process, and
  drives hive-engine as a subprocess.

hiveGo's `Board` is the sole arbiter: it validates every move and decides the
result. hive-engine is only ever asked "what would you play".

## Why raw coordinates on the wire are safe

The two engines independently chose the *same* axial convention — neighbour
deltas `(0,-1) (1,-1) (1,0) (0,1) (-1,1) (-1,0)` — and both force the first
placement to the origin and the second to its neighbours. So a position needs
no transform, and a move token is just a piece letter and coordinates.

That is a coincidence, not a contract, and a silent coordinate skew would still
produce a plausible-looking game. So `-validate` (on by default) asks both
engines for their full legal move list at every ply and fails the run on any
disagreement. Placements are compared by piece *type*: hive-engine emits one
`Place` per in-hand `PieceId` (three ants ⇒ three moves with identical effect),
hiveGo emits one per type, so both sides are deduplicated before comparing.

This doubles as a differential test of our move generator against a foreign
implementation, which is a stronger correctness signal than perft against
ourselves. `-fuzz N` runs it standalone: N random playouts, every ply checked,
no search, fully reproducible from `-seed`. Run it before trusting any match.

```bash
/tmp/hiveGo/hivebridge -rust target/release/examples/bridge \
  -fuzz 200 -seed 1 -max_moves 300
```

## The beetle-gate patch

The first thing `-validate` found was a rules bug — in hiveGo, not in us.

`internal/state/pieces.go` lets a ground beetle climb onto *any* occupied
neighbour, and gives a stacked beetle all six neighbours, with no check that it
can clear the two hexes flanking the step. Hive's freedom-to-move rule applies
at height: the step is blocked when both flanking stacks are strictly taller
than both the level the beetle drops to on the source hex and the level it
climbs to on the target. `engine/src/rules.rs:beetle_gate_allows` implements
this; hiveGo omits it.

A concrete disagreement (48 random playouts in, seed 7): a ground beetle at
(1,0) climbing onto a lone ant at (2,0), flanked by two 2-tile stacks at (2,-1)
and (1,1). Both gates are height 2, the source-after-leaving is 0 and the
target is 1, so the beetle cannot squeeze through. hiveGo offers the move; we
do not.

`bridge/hivego-beetle-gate.patch` adds the rule to hiveGo, and `setup.sh`
applies it. This is not optional cosmetics: without it the two engines are
playing different games, hiveGo will eventually play a move we consider
illegal, and no match result means anything. With it, 78k+ plies of random
playout agree exactly.

If the patch stops applying, hiveGo has changed `beetleMoves` upstream — check
whether the gate rule landed there before refreshing it.

## Setup

Needs a Go 1.26+ toolchain. `go` on `PATH`, or `GO=/path/to/go`.

```bash
bridge/setup.sh                 # clones to /tmp/hiveGo, builds both sides
bridge/setup.sh ~/src/hiveGo    # or somewhere that survives a reboot
```

The script removes hiveGo's `replace github.com/gomlx/gomlx => ../gomlx/gomlx`
directive, an author-local path that exists on no other machine and makes even
`go list` fail.

## Running

```bash
/tmp/hiveGo/hivebridge \
  -rust target/release/examples/bridge \
  -games 100 -think 100ms -ai 'linear,ab,max_time=100ms'
```

Colours swap every game and each game gets `-opening` random plies (seeded), so
the games actually differ — both engines are deterministic, and without a
randomised opening a "100 game match" is one game counted a hundred times.
Results are also broken out by seat, because Hive's first-player advantage is
large enough that a seat-blind score can be mostly seat luck.

Use an even `-games` so each side gets each seat equally often.

### Opponent configurations

| `-ai` | opponent |
|---|---|
| `linear,ab,max_time=100ms` | hand-tuned linear eval + alpha-beta |
| `fnn=#0,ab,max_time=100ms` | pretrained neural board eval + alpha-beta |
| `a0fnn=#0,mcts,max_time=100ms,temperature=0.2` | AlphaZero-style net + MCTS |

The latter two need the neural stack:

```bash
cd /tmp/hiveGo && go build -tags gomlx -o hivebridge-gomlx ./cmd/hivebridge
```

On first run this downloads XLA's CPU PJRT plugin (~66 MB) to `~/.cache/go-xla`
and installs it to `~/.local/lib/go-xla`.

## Results — 2026-09-17, 3 s/move

hiveGo's own default time control. 100 games per opponent, 3 s/move for both
sides, 4 random opening plies, colours swapped every game, 100-ply draw cap,
seed 1. Every ply of every game passed the move-generator cross-check.

| hiveGo opponent | hive-engine | hiveGo | draws | score | Elo |
|---|---|---|---|---|---|
| `linear,ab` | 62 (35 1st / 27 2nd) | 20 | 18 | **71.0% ± 4.0%** | +155 |
| `fnn=#0,ab` | 72 (41 1st / 31 2nd) | 15 | 13 | **78.5% ± 3.7%** | +225 |
| `a0fnn=#0,mcts` | 57 (30 1st / 27 2nd) | 22 | 21 | **67.5% ± 4.1%** | +127 |

All three are 4–8 standard errors above 50%, so hive-engine is clearly stronger
than every player hiveGo ships. Seat splits stay near-even.

**Thirty times the clock narrowed every margin**, by 7.5–15.5 points versus the
100 ms run below — so the short control was flattering us, and not only against
the neural players. Read the 3 s numbers as the honest ones.

**We convert time poorly.** 30× the budget bought ~1.6 extra plies (3.3–4.0 →
5.2–5.6) on 30× the nodes (~21k → ~650k per move). That is what a large
branching factor costs, and it is an argument for move-ordering and reduction
work over further evaluation tuning: an opponent that searches better will gain
more from a long control than we do.

**Draw structure shifted.** Repetition draws fell (16 → 12 of the total) but
cap draws rose (23 → 21, and 15 of a0fnn's 21 draws alone), because at 3 s both
sides are strong enough to grind out long balanced games. A higher cap would
resolve some of those and is worth a follow-up run.

## Results — 2026-09-16, 100 ms/move

100 games per opponent, 100 ms/move for both sides, 4 random opening plies,
colours swapped every game, hiveGo's default 100-ply draw cap, seed 1. Every
ply of every game passed the move-generator cross-check.

| hiveGo opponent | hive-engine | hiveGo | draws | score |
|---|---|---|---|---|
| `linear,ab` — hand-tuned linear eval | 78 (38 1st / 40 2nd) | 5 | 17 | **86.5%** |
| `fnn=#0,ab` — pretrained neural eval | 84 (45 1st / 39 2nd) | 2 | 14 | **91.0%** |
| `a0fnn=#0,mcts` — AlphaZero net + MCTS | 66 (37 1st / 29 2nd) | 16 | 18 | **75.0%** |

Superseded by the 3 s run above, which is the fairer control. Kept for the
time-scaling comparison. hive-engine reached depth 3.3–4.0 on ~18–23k nodes
per move.

**Draws are where our known gaps show up.** Across the three matches, 16 of the
49 draws were threefold repetition — half-points dropped because our search has
no repetition detection and will happily walk into a loop it cannot see, while
every opponent here can. Another 23 were the 100-ply cap. Both are the
repetition-detection follow-up in the root `CLAUDE.md`, now with a price tag
attached.

## Caveats

- **Draw cap.** hiveGo scores a draw at `-max_moves` plies (its default is 100)
  and on threefold repetition. hive-engine has neither rule, so in a drawn-by-
  cap game it was still playing to win while its opponent was content; the cap
  is applied by the arbiter, equally, to both.
- **Repetition.** hiveGo's search knows repetitions are draws and ours does not
  — the open follow-up in the root `CLAUDE.md`. This is a real handicap for us
  here, not an artifact of the harness.
- **Equal wall clock.** Both sides get the same per-move budget and the match
  is run sequentially, so a parallel run would distort both. Don't add
  parallelism without re-checking that.
