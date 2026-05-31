//! Hive UI — Leptos CSR entry point.
//!
//! Phase: scaffold (PR 1). Mounts a minimal app and constructs a
//! `hive_engine::State` to prove the engine dependency links and compiles to
//! WebAssembly. Board rendering, hand, move input, and search-on-a-worker land
//! in later PRs.

use hive_engine::{Color, State};
use leptos::*;

fn main() {
    // Route Rust panics to the browser devtools console.
    console_error_panic_hook::set_once();
    mount_to_body(App);
}

#[component]
fn App() -> impl IntoView {
    // Prove the engine links: build the initial position and read a couple of
    // public-API values for the placeholder. Nothing here mutates state yet.
    let state = State::new();
    let to_move = match state.side_to_move() {
        Color::White => "White",
        Color::Black => "Black",
    };
    let legal_moves = state.legal_moves().len();
    let placements = state.placements_so_far();

    view! {
        <main class="app">
            <h1>"Hive UI"</h1>
            <section class="board-placeholder" aria-label="board placeholder">
                <p>"Board renders here."</p>
            </section>
            <footer class="status">
                <span>"Side to move: " {to_move}</span>
                <span>"Legal moves: " {legal_moves}</span>
                <span>"Placements so far: " {placements}</span>
            </footer>
        </main>
    }
}
