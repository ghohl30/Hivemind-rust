//! Self-play strength testing: two evaluations, equal time, many games.
//!
//!     cargo run --release --example gauntlet -- [--openings N] [--ms MS] [--seed S]
//!
//! Defaults: 50 openings (= 100 games), 300ms per move, seed 1.
//!
//! Reports the win rate of `CurrentEval` against `LegacyEval`. This is the only
//! evidence that an evaluation change actually made the engine stronger — node
//! counts cannot tell you that, and a change can cut nodes while playing worse.
//!
//! Two design points make the number meaningful:
//!
//! - **Randomised openings.** Both players are deterministic, so every game from
//!   the initial position between the same two engines is the *same game*. N
//!   games from `State::new()` would be one game counted N times. Each opening
//!   here is a short random legal sequence, so the sample is of genuinely
//!   different positions.
//! - **Colour-swapped pairs.** Every opening is played twice with the engines on
//!   opposite sides, so a colour or opening advantage cancels instead of being
//!   read as a strength difference.
//!
//! Equal time rather than equal depth: a richer evaluation that reaches the same
//! depth more slowly has not improved anything, and only a time-based comparison
//! charges it for that.

use std::env;
use std::time::{Duration, Instant};

use hive_engine::{
    play_game_from, CurrentEval, LegacyEval, Move, Outcome, State, TimedSearchPlayer,
};

/// splitmix64. The crate has no `rand` dependency and its Zobrist mixer is
/// private, so the few bits of randomness needed here are inlined rather than
/// pulling a dependency into the workspace for one example.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed)
    }
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
}

/// Build an opening by playing `plies` uniformly random legal moves.
///
/// Kept short and even: long enough to diversify, short enough that the
/// positions stay sane rather than already decided, and even so both colours
/// have made the same number of moves when the engines take over.
fn random_opening(rng: &mut Rng, plies: usize) -> State {
    let mut s = State::new();
    for _ in 0..plies {
        if s.is_terminal().is_some() {
            break;
        }
        let moves: Vec<Move> = s.legal_moves().into_iter().collect();
        if moves.is_empty() {
            break;
        }
        let idx = rng.below(moves.len());
        s.apply(moves[idx]);
    }
    s
}

#[derive(Default, Clone, Copy)]
struct Tally {
    wins: u32,
    losses: u32,
    draws: u32,
}

impl Tally {
    fn games(&self) -> u32 {
        self.wins + self.losses + self.draws
    }
    /// Score rate with draws counted as a half point, the usual convention.
    fn score_rate(&self) -> f64 {
        if self.games() == 0 {
            return 0.0;
        }
        (self.wins as f64 + 0.5 * self.draws as f64) / self.games() as f64
    }
}

fn arg(name: &str) -> Option<String> {
    let args: Vec<String> = env::args().collect();
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

fn main() {
    let openings: usize = arg("--openings").and_then(|s| s.parse().ok()).unwrap_or(50);
    let ms: u64 = arg("--ms").and_then(|s| s.parse().ok()).unwrap_or(300);
    let seed: u64 = arg("--seed").and_then(|s| s.parse().ok()).unwrap_or(1);
    let opening_plies: usize = arg("--opening-plies").and_then(|s| s.parse().ok()).unwrap_or(6);
    const MAX_PLIES: u32 = 300;
    const TT_LOG2: u32 = 18;

    let budget = Duration::from_millis(ms);

    println!("gauntlet: CurrentEval vs LegacyEval");
    println!(
        "{} openings x 2 colours = {} games, {}ms/move, seed {}, {} random opening plies",
        openings,
        openings * 2,
        ms,
        seed,
        opening_plies
    );
    println!();

    let mut rng = Rng::new(seed);
    // As White, and as Black, tracked separately: a large gap between the two
    // means the harness is biased, not that one evaluation is better.
    let mut as_white = Tally::default();
    let mut as_black = Tally::default();
    let mut total_plies: u64 = 0;
    let mut truncated = 0u32;

    let started = Instant::now();

    for i in 0..openings {
        let opening = random_opening(&mut rng, opening_plies);
        if opening.is_terminal().is_some() {
            continue;
        }

        for swap in [false, true] {
            // Fresh players per game so no transposition table carries over and
            // silently advantages whoever moved second.
            let mut cur = TimedSearchPlayer::<CurrentEval>::new(budget, TT_LOG2);
            let mut leg = TimedSearchPlayer::<LegacyEval>::new(budget, TT_LOG2);

            let result = if swap {
                play_game_from(opening.clone(), &mut leg, &mut cur, MAX_PLIES)
            } else {
                play_game_from(opening.clone(), &mut cur, &mut leg, MAX_PLIES)
            };

            // Which colour was CurrentEval playing this game?
            let cur_is_white = !swap;
            let tally = if cur_is_white { &mut as_white } else { &mut as_black };

            match result.outcome {
                Outcome::Draw => tally.draws += 1,
                Outcome::Win(c) => {
                    let cur_won = (c == hive_engine::Color::White) == cur_is_white;
                    if cur_won {
                        tally.wins += 1;
                    } else {
                        tally.losses += 1;
                    }
                }
            }
            total_plies += result.plies as u64;
            if result.truncated {
                truncated += 1;
            }
        }

        let done = (i + 1) * 2;
        if done % 20 == 0 {
            let combined = as_white.score_rate() * 0.5 + as_black.score_rate() * 0.5;
            println!(
                "  {done:>4} games   current scoring {:.1}%   ({:.0?} elapsed)",
                combined * 100.0,
                started.elapsed()
            );
        }
    }

    let games = as_white.games() + as_black.games();
    let wins = as_white.wins + as_black.wins;
    let losses = as_white.losses + as_black.losses;
    let draws = as_white.draws + as_black.draws;
    let score = (wins as f64 + 0.5 * draws as f64) / games as f64;
    // Binomial standard error on the score rate, the usual rough guide to
    // whether a result is distinguishable from 50%.
    let stderr = (0.25f64 / games as f64).sqrt();

    println!();
    println!("{:->64}", "");
    println!("games              {games}");
    println!("current W/L/D      {wins} / {losses} / {draws}");
    println!(
        "  as white         {} / {} / {}   ({:.1}%)",
        as_white.wins,
        as_white.losses,
        as_white.draws,
        as_white.score_rate() * 100.0
    );
    println!(
        "  as black         {} / {} / {}   ({:.1}%)",
        as_black.wins,
        as_black.losses,
        as_black.draws,
        as_black.score_rate() * 100.0
    );
    println!(
        "current score      {:.1}%  (+/- {:.1}% 1 s.e.)",
        score * 100.0,
        stderr * 100.0
    );
    println!("avg game length    {:.0} plies", total_plies as f64 / games as f64);
    println!("truncated          {truncated}");
    println!("wall clock         {:.0?}", started.elapsed());
    println!();
    let z = (score - 0.5) / stderr;
    if z.abs() < 2.0 {
        println!("verdict: not distinguishable from 50% (|z| = {:.1} < 2)", z.abs());
    } else if z > 0.0 {
        println!("verdict: current is stronger (z = +{z:.1})");
    } else {
        println!("verdict: current is WEAKER (z = {z:.1})");
    }
}
