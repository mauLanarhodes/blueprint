//! Workspace settings live in eframe storage, never in a Blueprint document.
use bp_model::{CloudProvider, ShapeRef};
use serde::{Deserialize, Serialize};

const STORAGE_KEY: &str = "blueprint.workspace";
const VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceTheme {
    #[default]
    Dark,
    Light,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LeftTab {
    #[default]
    Library,
    Navigator,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogView {
    #[default]
    Grid,
    List,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkspacePreferences {
    pub version: u32,
    pub theme: WorkspaceTheme,
    pub left_width: f32,
    pub right_width: f32,
    pub left_open: bool,
    pub right_open: bool,
    pub left_tab: LeftTab,
    pub drawer_open: bool,
    pub drawer_height: f32,
    pub show_grid: bool,
    pub snap_to_grid: bool,
    pub snap_to_shapes: bool,
    pub grid: f64,
    pub cloud_provider: CloudProvider,
    pub cloud_version: Option<String>,
    pub cloud_view: CatalogView,
    pub cloud_category: Option<String>,
    pub favorites: Vec<ShapeRef>,
    pub recent: Vec<ShapeRef>,
    pub collapsed_libraries: Vec<String>,
}

impl Default for WorkspacePreferences {
    fn default() -> Self {
        Self {
            version: VERSION,
            theme: WorkspaceTheme::Dark,
            left_width: 256.0,
            right_width: 304.0,
            left_open: true,
            right_open: true,
            left_tab: LeftTab::Library,
            drawer_open: true,
            drawer_height: 176.0,
            show_grid: true,
            snap_to_grid: true,
            snap_to_shapes: true,
            grid: 10.0,
            cloud_provider: CloudProvider::Aws,
            cloud_version: None,
            cloud_view: CatalogView::Grid,
            cloud_category: None,
            favorites: Vec::new(),
            recent: Vec::new(),
            collapsed_libraries: Vec::new(),
        }
    }
}

impl WorkspacePreferences {
    pub fn load(storage: Option<&dyn eframe::Storage>) -> Self {
        storage
            .and_then(|s| s.get_string(STORAGE_KEY))
            .and_then(|s| Self::from_json(&s))
            .unwrap_or_default()
    }
    pub fn from_json(json: &str) -> Option<Self> {
        let mut prefs: Self = serde_json::from_str(json).ok()?;
        if prefs.version != VERSION {
            return None;
        }
        prefs.sanitize();
        Some(prefs)
    }
    pub fn store(&self, storage: &mut dyn eframe::Storage) {
        if let Ok(json) = serde_json::to_string(self) {
            storage.set_string(STORAGE_KEY, json);
        }
    }
    pub(crate) fn sanitize(&mut self) {
        let defaults = Self::default();
        fn bounded(value: f32, default: f32, min: f32, max: f32) -> f32 {
            if value.is_finite() {
                value.clamp(min, max)
            } else {
                default
            }
        }
        self.left_width = bounded(self.left_width, defaults.left_width, 220.0, 400.0);
        self.right_width = bounded(self.right_width, defaults.right_width, 260.0, 440.0);
        self.drawer_height = bounded(self.drawer_height, defaults.drawer_height, 120.0, 360.0);
        self.grid = if self.grid.is_finite() {
            self.grid.clamp(2.0, 200.0)
        } else {
            defaults.grid
        };
        let mut seen = std::collections::HashSet::new();
        self.recent.retain(|r| seen.insert(r.clone()));
        self.recent.truncate(12);
        seen.clear();
        self.favorites.retain(|r| seen.insert(r.clone()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trip_and_invalid_preferences() {
        let p = WorkspacePreferences {
            theme: WorkspaceTheme::Light,
            left_open: false,
            ..Default::default()
        };
        assert_eq!(
            WorkspacePreferences::from_json(&serde_json::to_string(&p).unwrap()),
            Some(p)
        );
        assert!(WorkspacePreferences::from_json("not json").is_none());
        assert!(WorkspacePreferences::from_json(r#"{"version":99}"#).is_none());
        let p = WorkspacePreferences::from_json(r#"{"left_width":9000,"right_width":2,"grid":0}"#)
            .unwrap();
        assert_eq!((p.left_width, p.right_width, p.grid), (400.0, 260.0, 2.0));
    }
}
