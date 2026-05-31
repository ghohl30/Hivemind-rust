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

/// AI strength preset. Each maps to a fixed negamax search depth passed to
/// `hive_engine::search::search`.
///
/// Depths are chosen to stay responsive in a WASM Web Worker while giving a
/// meaningful strength gradient:
///   - `Easy`   = depth 2 — shallow, near-instant, beatable by a beginner.
///   - `Medium` = depth 4 — the engine's own `search_bench` default; a solid
///     club-level opponent that still returns promptly.
///   - `Hard`   = depth 6 — noticeably stronger; the upper end we trust to stay
///     interactive before the engine ships a time-bounded entry point
///     (engine-request #1), at which point these become a fallback.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Difficulty {
    Easy,
    Medium,
    Hard,
}

impl Difficulty {
    /// The concrete search depth for this preset.
    pub fn depth(self) -> u8 {
        match self {
            Difficulty::Easy => 2,
            Difficulty::Medium => 4,
            Difficulty::Hard => 6,
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
    /// The search depth the AI should use, derived from the difficulty preset.
    pub fn ai_depth(self) -> u8 {
        self.difficulty.depth()
    }

    /// Whether the AI moves first (i.e. it plays White, who always opens).
    pub fn ai_moves_first(self) -> bool {
        self.ai == Color::White
    }
}
