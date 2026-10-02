//! The Blueprint desktop app. `main.rs` only starts it; everything lives
//! here so UI tests can drive the real app.

mod actions;
mod app;
mod canvas;
mod dialogs;
mod inspector;
mod menus;
mod pages;
mod palette;
mod selection;
mod theme;

pub use app::{BlueprintApp, Tool};
