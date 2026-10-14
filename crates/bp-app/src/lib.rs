//! The Blueprint desktop app. `main.rs` only starts it; everything lives
//! here so UI tests can drive the real app.

mod actions;
mod app;
mod calendar;
mod canvas;
mod cloud;
mod connections;
mod deployment;
mod dialogs;
mod erd;
mod erd_focus;
mod inspector;
mod inspector_sections;
mod menus;
mod navigator;
mod pages;
mod palette;
mod preferences;
mod presets;
mod selection;
mod sql;
mod sql_inspector;
mod theme;
mod widgets;
mod workspace;

pub use app::{BlueprintApp, PageChoice, Tool};
pub use cloud::CloudState;
pub use connections::ErdConnection;
pub use deployment::DeploymentState;
pub use erd_focus::ErdFocus;
pub use inspector_sections::InspectorTab;
pub use preferences::{CatalogView, LeftTab, WorkspacePreferences, WorkspaceTheme};
pub use sql::{SqlExport, SqlImport, SqlState};
pub use workspace::WorkspaceState;
