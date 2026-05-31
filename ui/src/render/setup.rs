//! New-game setup screen: color picker, difficulty picker, Start button.

use js_sys::Math;
use leptos::*;

use crate::game::{Difficulty, GameSetup, HumanColor, NewGameConfig};

/// Setup screen shown before a game begins. Calls `on_start` with the resolved
/// [`GameSetup`] when the player clicks "Start Game".
#[component]
pub fn SetupScreen(on_start: Callback<GameSetup>) -> impl IntoView {
    let (human_color, set_human_color) = create_signal(HumanColor::White);
    let (difficulty, set_difficulty) = create_signal(Difficulty::Medium);

    let start = move |_| {
        let coin = Math::random() >= 0.5;
        let config = NewGameConfig {
            human: human_color.get(),
            difficulty: difficulty.get(),
        };
        on_start.call(config.resolve(coin));
    };

    view! {
        <section class="setup" aria-label="New game setup">
            <h2 class="setup-title">"New Game"</h2>

            <div class="setup-row">
                <span class="eyebrow">"Play as"</span>
                <div class="setup-opts" role="group" aria-label="Color">
                    <button
                        class=move || if human_color.get() == HumanColor::White { "opt-btn active" } else { "opt-btn" }
                        on:click=move |_| set_human_color.set(HumanColor::White)
                    >"White"</button>
                    <button
                        class=move || if human_color.get() == HumanColor::Black { "opt-btn active" } else { "opt-btn" }
                        on:click=move |_| set_human_color.set(HumanColor::Black)
                    >"Black"</button>
                    <button
                        class=move || if human_color.get() == HumanColor::Random { "opt-btn active" } else { "opt-btn" }
                        on:click=move |_| set_human_color.set(HumanColor::Random)
                    >"Random"</button>
                </div>
            </div>

            <div class="setup-row">
                <span class="eyebrow">"Difficulty"</span>
                <div class="setup-opts" role="group" aria-label="Difficulty">
                    <button
                        class=move || if difficulty.get() == Difficulty::Easy { "opt-btn active" } else { "opt-btn" }
                        on:click=move |_| set_difficulty.set(Difficulty::Easy)
                    >"Easy"</button>
                    <button
                        class=move || if difficulty.get() == Difficulty::Medium { "opt-btn active" } else { "opt-btn" }
                        on:click=move |_| set_difficulty.set(Difficulty::Medium)
                    >"Medium"</button>
                    <button
                        class=move || if difficulty.get() == Difficulty::Hard { "opt-btn active" } else { "opt-btn" }
                        on:click=move |_| set_difficulty.set(Difficulty::Hard)
                    >"Hard"</button>
                </div>
            </div>

            <button class="start-btn" on:click=start>"Start Game"</button>
        </section>
    }
}
