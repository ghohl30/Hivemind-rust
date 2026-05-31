//! 10-game match: SearchPlayer(depth=3) as White vs SearchPlayer(depth=2) as Black.
//!
//! Run with:
//!   cargo run --release --example match -p hive-engine

use hive_engine::{game_runner::play_game, player::SearchPlayer, state::Outcome, Color};

const GAMES: u32 = 10;
const MAX_PLIES: u32 = 300;
const TT_LOG2: u32 = 16;

fn main() {
    let mut white_wins = 0u32;
    let mut black_wins = 0u32;
    let mut draws = 0u32;

    for game in 1..=GAMES {
        // Fresh players each game — no TT bleed between games.
        let mut white = SearchPlayer::new(3, TT_LOG2);
        let mut black = SearchPlayer::new(2, TT_LOG2);

        let result = play_game(&mut white, &mut black, MAX_PLIES);

        let label = match result.outcome {
            Outcome::Win(Color::White) => {
                white_wins += 1;
                "White wins"
            }
            Outcome::Win(Color::Black) => {
                black_wins += 1;
                "Black wins"
            }
            Outcome::Draw => {
                draws += 1;
                if result.truncated { "Draw (truncated)" } else { "Draw" }
            }
        };

        println!(
            "Game {:2}: {} in {} plies{}",
            game,
            label,
            result.plies,
            if result.truncated { " [cap hit]" } else { "" }
        );
    }

    println!();
    println!("--- Match result ({GAMES} games) ---");
    println!("  White (depth 3): {white_wins} wins");
    println!("  Black (depth 2): {black_wins} wins");
    println!("  Draws:           {draws}");
}
