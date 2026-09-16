//! Line protocol over stdin/stdout so a foreign engine can drive this one.
//!
//! Written for the hiveGo bridge (`bridge/` at the workspace root), but the
//! protocol is engine-agnostic: the peer owns the game loop and the clock, we
//! own nothing but a `State` and a transposition table.
//!
//! Move tokens use hiveGo's piece letters and *raw axial coordinates*, which
//! works only because both engines happen to use the identical axial
//! convention — E (+1,0), W (-1,0), SE (0,+1), NW (0,-1), NE (+1,-1),
//! SW (-1,+1) — and both force the first placement to the origin. There is no
//! transform. If either side ever changes its convention, every token silently
//! becomes wrong, so `bridge/` cross-checks the two move generators ply by ply
//! rather than trusting this comment.
//!
//!   ->  init                 <-  ok
//!   ->  moves                <-  moves <tok> <tok> ...   (deduped, sorted)
//!   ->  apply <tok>          <-  ok
//!   ->  go <millis>          <-  bestmove <tok> depth <d> nodes <n>
//!   ->  result               <-  result white|black|draw|none
//!   ->  quit                 <-  (exits)
//!
//! Tokens: `P<letter><q>,<r>` place, `M<q>,<r>,<q>,<r>` slide, `X` pass.

use std::collections::BTreeSet;
use std::io::{self, BufRead, Write};
use std::time::Duration;

use hive_engine::{Color, Move, Outcome, PieceType, State, TranspositionTable};

/// hiveGo's `PieceLetters`, so tokens are readable on both sides of the pipe.
fn letter(t: PieceType) -> char {
    match t {
        PieceType::SoldierAnt => 'A',
        PieceType::Beetle => 'B',
        PieceType::Grasshopper => 'G',
        PieceType::QueenBee => 'Q',
        PieceType::Spider => 'S',
        other => panic!("expansion piece {other:?} has no hiveGo letter"),
    }
}

/// Render a move as a token. Placements are keyed by piece *type*, not
/// `PieceId`: our generator emits one `Place` per in-hand piece, so the three
/// ants produce three distinct moves with identical effect. Collapsing them
/// here is what makes our move list comparable to hiveGo's, which dedupes by
/// type.
fn encode(state: &State, m: Move) -> String {
    match m {
        Move::Place { piece, to } => format!("P{}{},{}", letter(piece.piece_type()), to.q, to.r),
        Move::Slide { piece, to } => {
            let from = state
                .piece_slot(piece)
                .coord()
                .expect("a sliding piece is on the board");
            format!("M{},{},{},{}", from.q, from.r, to.q, to.r)
        }
        Move::Pass => "X".to_string(),
    }
}

/// Resolve a token back to a legal move. Round-tripping through `encode` over
/// the legal list means the two directions cannot disagree, and an illegal or
/// malformed token simply finds no match instead of corrupting the state.
fn decode(state: &State, tok: &str) -> Option<Move> {
    state
        .legal_moves()
        .into_iter()
        .find(|m| encode(state, *m) == tok)
}

fn main() -> io::Result<()> {
    let stdin = io::stdin();
    let mut out = io::stdout();
    let mut state = State::new();
    // One table for the whole game, as `SearchPlayer` does — carrying it
    // across moves is worth real ordering quality.
    let mut tt = TranspositionTable::with_capacity_log2(20);

    for line in stdin.lock().lines() {
        let line = line?;
        let line = line.trim();
        let (cmd, arg) = match line.split_once(' ') {
            Some((c, a)) => (c, a.trim()),
            None => (line, ""),
        };

        match cmd {
            "" => continue,
            "init" => {
                state = State::new();
                tt = TranspositionTable::with_capacity_log2(20);
                writeln!(out, "ok")?;
            }
            "moves" => {
                let toks: BTreeSet<String> = state
                    .legal_moves()
                    .into_iter()
                    .map(|m| encode(&state, m))
                    .collect();
                writeln!(
                    out,
                    "moves {}",
                    toks.into_iter().collect::<Vec<_>>().join(" ")
                )?;
            }
            "apply" => match decode(&state, arg) {
                Some(m) => {
                    state.apply(m);
                    writeln!(out, "ok")?;
                }
                None => writeln!(out, "error illegal move {arg}")?,
            },
            "go" => {
                let ms: u64 = arg.parse().unwrap_or(1000);
                let mut working = state.clone();
                let (_score, best, stats) = hive_engine::search_timed(
                    &mut working,
                    64,
                    &mut tt,
                    Duration::from_millis(ms),
                );
                match best {
                    Some(m) => writeln!(
                        out,
                        "bestmove {} depth {} nodes {}",
                        encode(&state, m),
                        stats.depth,
                        stats.nodes
                    )?,
                    None => writeln!(out, "error no move in terminal position")?,
                }
            }
            "result" => {
                let s = match state.is_terminal() {
                    Some(Outcome::Win(Color::White)) => "white",
                    Some(Outcome::Win(Color::Black)) => "black",
                    Some(Outcome::Draw) => "draw",
                    None => "none",
                };
                writeln!(out, "result {s}")?;
            }
            "quit" => break,
            other => writeln!(out, "error unknown command {other}")?,
        }
        out.flush()?;
    }
    Ok(())
}
