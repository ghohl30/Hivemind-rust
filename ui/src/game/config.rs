//! New-game configuration: who the human plays, how strong the AI is, and which
//! side the engine controls.
//!
//! White always moves first regardless of which color the human takes — that is
//! the rules of Hive, not a UI choice.

use hive_engine::Color;

/// The color the human wants to play. `Random` is resolved once at game start
/// via [`HumanColor::resolve`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum HumanColor {
    White,
    Black,
    Random,
}

impl HumanColor {
    /// Resolve to a concrete [`Color`], using `coin` to decide a `Random`
    /// choice. `coin == false` ⇒ White, `coin == true` ⇒ Black. Taking the coin
    /// as a parameter keeps this deterministic and testable; the caller supplies
    /// randomness (e.g. `js_sys::Math::random() >= 0.5` in the browser).
    pub fn resolve(self, coin: bool) -> Color {
        match self {
            HumanColor::White => Color::White,
            HumanColor::Black => Color::Black,
            HumanColor::Random => {
                if coin {
                    Color::Black
                } else {
                    Color::White
                }
            }
        }
    }
}

/// Depth ceiling handed to `hive_engine::search::search_bounded`.
///
/// The wall-clock budget is the real limit; this only stops iterative deepening
/// from running past a depth no Hive position reaches inside our longest budget.
/// It exists so a forced line that searches absurdly fast cannot spin.
pub const MAX_DEPTH: u8 = 12;

/// AI strength preset. Each maps to a wall-clock think-time budget spent by
/// `hive_engine::search::search_bounded`, which iteratively deepens until the
/// budget expires and returns the best move from the last *completed* iteration.
///
/// Time, not depth, is the control: Hive's branching factor swings from ~10 in
/// the opening to 80+ once ants are out, so a fixed depth that returns instantly
/// on move 3 can take minutes on move 30. Budgets follow the engine's own
/// recommendation when it shipped the entry point (engine-request #1, PR #18):
///   - `Easy`   = 0.5 s — near-instant, beatable by a beginner.
///   - `Medium` = 3 s — a solid club-level opponent that still answers promptly.
///   - `Hard`   = 20 s — the strongest setting; the engine reaches roughly depth
///     8 from a midgame position in this budget.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Difficulty {
    Easy,
    Medium,
    Hard,
}

impl Difficulty {
    /// The think-time budget for this preset, in milliseconds.
    pub fn budget_ms(self) -> f64 {
        match self {
            Difficulty::Easy => 500.0,
            Difficulty::Medium => 3_000.0,
            Difficulty::Hard => 20_000.0,
        }
    }
}

/// Unresolved new-game settings as chosen in the menu.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct NewGameConfig {
    /// Color the human picked (possibly `Random`).
    pub human: HumanColor,
    /// AI strength preset.
    pub difficulty: Difficulty,
}

impl Default for NewGameConfig {
    fn default() -> Self {
        Self {
            human: HumanColor::White,
            difficulty: Difficulty::Medium,
        }
    }
}

impl NewGameConfig {
    /// Resolve into a concrete [`GameSetup`] for one game, settling `Random`
    /// with the supplied coin flip. White moves first regardless.
    pub fn resolve(self, coin: bool) -> GameSetup {
        let human = self.human.resolve(coin);
        GameSetup {
            human,
            ai: human.other(),
            difficulty: self.difficulty,
        }
    }
}

/// A resolved game setup: every color is concrete. Produced by
/// [`NewGameConfig::resolve`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GameSetup {
    /// The concrete color the human plays.
    pub human: Color,
    /// The concrete color the AI plays (always the other side).
    pub ai: Color,
    /// AI strength.
    pub difficulty: Difficulty,
}

impl GameSetup {
    /// The think-time budget the AI should use, derived from the difficulty
    /// preset, in milliseconds.
    pub fn ai_budget_ms(self) -> f64 {
        self.difficulty.budget_ms()
    }

    /// Whether the AI moves first (i.e. it plays White, who always opens).
    pub fn ai_moves_first(self) -> bool {
        self.ai == Color::White
    }
}
