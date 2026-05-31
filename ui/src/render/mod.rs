//! Render layer for the Hive UI.
//!
//! Split into pure (native-testable) and component (Leptos/CSR) pieces:
//!   - [`hex`]   — flat-top axial→pixel transform, hex polygon, viewBox fit.
//!   - [`view`]  — engine-type → asset/color mapping and derived view-models.
//!   - [`demo`]  — the fixed demo session this PR renders.
//!   - [`components`] — the Leptos components that turn the above into markup.
//!
//! `hex`, `view`, and `demo` have no DOM dependency and carry the unit tests;
//! `components` is the thin presentational translation layer.

pub mod demo;
pub mod hex;
pub mod interaction;
pub mod view;

pub mod components;

pub use components::App;
