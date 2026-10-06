//! The Blueprint desktop app. `main.rs` only starts it; everything lives
//! here so UI tests can drive the real app.

mod actions;
mod app;
mod calendar;
mod canvas;
mod cloud;
mod connections;
mod dialogs;
mod erd;
mod inspector;
mod menus;
mod pages;
mod palette;
mod selection;
mod sql;
mod sql_inspector;
mod theme;

pub use app::{BlueprintApp, PageChoice, Tool};
pub use cloud::CloudState;
pub use connections::ErdConnection;
pub use sql::{SqlExport, SqlImport, SqlState};
