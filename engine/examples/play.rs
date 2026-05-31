//! Minimal deterministic play loop — a reference for how a UI would drive
//! the engine. No user input; always picks the first legal move so the game
//! runs to completion automatically.
//!
//! Run with: `cargo run --release --example play -p hive-engine`

use hive_engine::{Move, Outcome, PieceId, PieceSlot, State};

fn main() {
    let mut state = State::new();
    let mut ply = 0u32;

    loop {
        // Check for game over first.
        if let Some(outcome) = state.is_terminal() {
            println!(
                "Game over after {} plies: {}",
                ply,
                match outcome {
                    Outcome::Win(c) => format!("{c:?} wins"),
                    Outcome::Draw => "Draw".to_string(),
                }
            );
            break;
        }

        let side = state.side_to_move();
        let turn = state.turn_for(side);
        let moves = state.legal_moves();

        // Count in-hand pieces for the side to move.
        let in_hand = PieceId::for_color(side)
            .filter(|&id| matches!(state.piece_slot(id), PieceSlot::InHand))
            .count();

        println!(
            "Ply {ply:>3} | {side:?} turn {turn} | in hand: {in_hand} | {} moves",
            moves.len()
        );

        // Pick the first legal move (deterministic, no randomness needed).
        let chosen = moves[0];
        match chosen {
            Move::Place { piece, to } => {
                println!("  -> Place {:?}({}) at ({},{})", piece.piece_type(), piece.0, to.q, to.r);
            }
            Move::Slide { piece, to } => {
                println!("  -> Slide {:?}({}) to ({},{})", piece.piece_type(), piece.0, to.q, to.r);
            }
            Move::Pass => {
                println!("  -> Pass");
            }
        }

        state.apply(chosen);
        ply += 1;

        // Safety cap: base-game draws are theoretically possible but rare in
        // deterministic greedy play. Stop at 200 plies to keep the example
        // output bounded.
        if ply >= 200 {
            println!("Reached ply cap (200) without terminal — stopping.");
            break;
        }
    }
}
