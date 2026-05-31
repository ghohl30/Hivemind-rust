//! Hive UI library crate.
//!
//! The binary entry point lives in `main.rs` (Leptos CSR → WebAssembly). This
//! library exposes the engine-facing game core, which is pure Rust and testable
//! on the native target.

pub mod game;
