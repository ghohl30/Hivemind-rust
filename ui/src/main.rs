//! Hive UI — Leptos CSR entry point.
//!
//! PR (board render): mounts the static board render of a fixed demo session.
//! No interaction yet — clicking/selecting/moving lands in a later PR.

use hive_ui::render::App;
use leptos::*;

fn main() {
    // Route Rust panics to the browser devtools console.
    console_error_panic_hook::set_once();
    mount_to_body(App);
}
