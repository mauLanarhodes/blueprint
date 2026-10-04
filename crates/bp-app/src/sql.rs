//! Preview-first SQL schema import and export. SQL is never executed.

use crate::app::{BlueprintApp, Tool};
use bp_model::{DiagramKind, LayerId, PageId, Parent, SqlDialect};
use bp_sql::{ExportPreview, ImportPreview, SUPPORTED_DIALECTS, Warning};
use std::path::{Path, PathBuf};

#[derive(Default)]
pub struct SqlState {
    pub import: Option<SqlImport>,
    pub export: Option<SqlExport>,
    /// Source scripts are protected even after their preview is dismissed.
    source_paths: Vec<PathBuf>,
}

impl SqlState {
    pub fn is_open(&self) -> bool {
        self.import.is_some() || self.export.is_some()
    }
}

pub struct SqlImport {
    pub source: String,
    pub source_path: Option<PathBuf>,
    pub dialect: SqlDialect,
    pub preview: Option<ImportPreview>,
    pub error: Option<String>,
    preview_source: String,
    preview_dialect: SqlDialect,
    page: PageId,
    layer: LayerId,
    focus_source: bool,
}

impl SqlImport {
    fn invalidate_changed_preview(&mut self) {
        if self.source != self.preview_source || self.dialect != self.preview_dialect {
            self.preview = None;
            self.error = None;
        }
    }
}

pub struct SqlExport {
    pub dialect: SqlDialect,
    pub preview: ExportPreview,
    page: PageId,
    revision: u64,
}

impl BlueprintApp {
    fn prepare_sql_dialog(&mut self) -> bool {
        if self.page_kind() != Some(DiagramKind::Erd) {
            self.error = Some("SQL schema import and export require an ERD page.".into());
            return false;
        }
        self.finish_inline_edits();
        self.cancel_drag();
        self.history.commit();
        self.quick_insert = None;
        self.palette.dragging = None;
        self.tool = Tool::Select;
        true
    }

    /// Starts an editable draft. Pasting uses the ordinary text editor clipboard.
    pub fn paste_sql_schema(&mut self) {
        self.start_sql_import(String::new(), None);
    }

    pub fn open_sql_file(&mut self) {
        if self.page_kind() != Some(DiagramKind::Erd) {
            return;
        }
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("SQL schema", &["sql"])
            .add_filter("All files", &["*"])
            .pick_file()
        {
            self.open_sql_path(&path);
        }
    }

    /// Reads a source script without adopting it as the Blueprint project path.
    pub fn open_sql_path(&mut self, path: &Path) {
        match std::fs::read_to_string(path) {
            Ok(source) => self.start_sql_import(source, Some(path.to_owned())),
            Err(error) => {
                self.error = Some(format!("Could not read {}:\n{error}", path.display()));
            }
        }
    }

    fn start_sql_import(&mut self, source: String, source_path: Option<PathBuf>) {
        if !self.prepare_sql_dialog() {
            return;
        }
        if let Some(path) = &source_path {
            self.sql.source_paths.push(path.clone());
        }
        self.sql.export = None;
        self.sql.import = Some(SqlImport {
            source,
            source_path,
            dialect: SqlDialect::PostgreSql,
            preview: None,
            error: None,
            preview_source: String::new(),
            preview_dialect: SqlDialect::PostgreSql,
            page: self.page,
            layer: self.layer,
            focus_source: true,
        });
    }

    pub fn preview_sql_import(&mut self) {
        let Some(draft) = &mut self.sql.import else {
            return;
        };
        draft.preview_source.clone_from(&draft.source);
        draft.preview_dialect = draft.dialect;
        match bp_sql::parse(&draft.source, draft.dialect) {
            Ok(preview) => {
                draft.preview = Some(preview);
                draft.error = None;
            }
            Err(error) => {
                draft.preview = None;
                draft.error = Some(error);
            }
        }
    }

    /// Imports tables and relationships together, separate from previous edits.
    pub fn apply_sql_import(&mut self) -> bool {
        let Some(draft) = &mut self.sql.import else {
            return false;
        };
        draft.invalidate_changed_preview();
        let Some(preview) = draft.preview.clone() else {
            return false;
        };
        if preview.schema.tables.is_empty() {
            return false;
        }
        let (page, layer) = (draft.page, draft.layer);
        if self.doc.pages.get(&page).and_then(|p| p.diagram_kind) != Some(DiagramKind::Erd)
            || !self
                .doc
                .layers
                .get(&layer)
                .is_some_and(|l| l.page == page && !l.locked)
        {
            draft.error = Some("Choose an unlocked layer on an ERD page before importing.".into());
            return false;
        }
        let (ids, commands) =
            match bp_sql::import_commands(&self.doc, Parent::Layer(layer), &preview) {
                Ok(result) => result,
                Err(error) => {
                    draft.error = Some(error);
                    return false;
                }
            };
        self.finish_inline_edits();
        self.cancel_drag();
        self.history.commit();
        if !self.apply("Import SQL schema", commands) {
            if let Some(draft) = &mut self.sql.import {
                draft.error = Some(self.status.clone());
            }
            return false;
        }
        self.history.commit();
        self.set_page(page);
        self.layer = layer;
        self.selection = ids;
        self.scope = None;
        self.fit_requested = true;
        self.status = format!("Imported {} SQL tables", preview.schema.tables.len());
        self.sql.import = None;
        true
    }

    pub fn open_sql_export(&mut self) {
        if !self.prepare_sql_dialog() {
            return;
        }
        match bp_sql::export_page(&self.doc, self.page, SqlDialect::PostgreSql) {
            Ok(preview) => {
                self.sql.import = None;
                self.sql.export = Some(SqlExport {
                    dialect: SqlDialect::PostgreSql,
                    preview,
                    page: self.page,
                    revision: self.history.revision(),
                });
            }
            Err(error) => self.error = Some(error),
        }
    }

    /// Saves the reviewed DDL without changing the current Blueprint save path.
    pub fn save_sql_to(&mut self, path: &Path) -> bool {
        let Some(export) = &self.sql.export else {
            return false;
        };
        if self
            .sql
            .source_paths
            .iter()
            .any(|source| same_file(path, source))
        {
            self.error = Some(
                "Choose a different SQL output file to preserve the original source script.".into(),
            );
            return false;
        }
        match bp_io::write_sql(path, &export.preview.sql, self.path.as_deref()) {
            Ok(()) => {
                self.status = format!("Exported {}", path.display());
                true
            }
            Err(error) => {
                self.error = Some(format!("Could not export {}:\n{error}", path.display()));
                false
            }
        }
    }

    pub(crate) fn sql_dialogs(&mut self, ctx: &egui::Context) {
        if self.pending.is_some() || self.error.is_some() || self.page_choice.is_some() {
            return;
        }
        self.sql_import_dialog(ctx);
        self.sql_export_dialog(ctx);
    }

    fn sql_import_dialog(&mut self, ctx: &egui::Context) {
        let Some(draft) = &mut self.sql.import else {
            return;
        };
        draft.invalidate_changed_preview();
        let mut preview = false;
        let mut apply = false;
        let mut cancel = false;
        let modal = egui::Modal::new(egui::Id::new("sql-import")).show(ctx, |ui| {
            ui.set_width(700.0_f32.min(ctx.content_rect().width() - 48.0));
            ui.heading("Import SQL schema");
            ui.label("Import PostgreSQL tables, keys, foreign keys, and indexes. Other SQL is reported as warnings. SQL is never executed.");
            if let Some(path) = &draft.source_path {
                ui.label(egui::RichText::new(format!("Source: {} (preserved)", path.display())).small());
            }
            dialect_picker(ui, "sql-import-dialect", &mut draft.dialect);
            let label = ui.label("SQL script");
            egui::ScrollArea::vertical().max_height(240.0).show(ui, |ui| {
                let response = ui.add(
                    egui::TextEdit::multiline(&mut draft.source)
                        .id_salt("sql-source")
                        .code_editor()
                        .desired_rows(10)
                        .desired_width(f32::INFINITY),
                ).labelled_by(label.id);
                if draft.focus_source {
                    response.request_focus();
                    draft.focus_source = false;
                }
            });
            draft.invalidate_changed_preview();
            if let Some(result) = &draft.preview {
                let columns: usize = result.schema.tables.iter().map(|t| t.columns.len()).sum();
                let tables = result.schema.tables.len();
                let relationships = result.schema.foreign_keys.len();
                ui.label(egui::RichText::new(format!(
                    "{tables} table{}, {columns} column{}, {relationships} relationship{}",
                    if tables == 1 { "" } else { "s" },
                    if columns == 1 { "" } else { "s" },
                    if relationships == 1 { "" } else { "s" },
                )).strong());
                if result.schema.tables.is_empty() {
                    ui.label("No supported tables were found. Edit the script and preview again.");
                }
                warnings(ui, &result.warnings);
            } else {
                ui.label("Preview the current script before applying it.");
            }
            if let Some(error) = &draft.error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                preview = ui.add_enabled(!draft.source.trim().is_empty(), egui::Button::new("Preview schema")).clicked();
                apply = ui.add_enabled(
                    draft.preview.as_ref().is_some_and(|p| !p.schema.tables.is_empty()),
                    egui::Button::new("Apply import"),
                ).clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
        if cancel || modal.should_close() {
            self.sql.import = None;
        } else if preview {
            self.preview_sql_import();
        } else if apply {
            self.apply_sql_import();
        }
    }

    fn sql_export_dialog(&mut self, ctx: &egui::Context) {
        let Some(export) = &mut self.sql.export else {
            return;
        };
        let previous_dialect = export.dialect;
        let mut save = false;
        let mut close = false;
        let modal = egui::Modal::new(egui::Id::new("sql-export")).show(ctx, |ui| {
            ui.set_width(700.0_f32.min(ctx.content_rect().width() - 48.0));
            ui.heading("Export page as SQL");
            dialect_picker(ui, "sql-export-dialect", &mut export.dialect);
            ui.label("Tables are created before foreign keys. Review warnings before saving a separate .sql file.");
            warnings(ui, &export.preview.warnings);
            let label = ui.label("SQL DDL preview");
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                // A borrowed immutable string makes the preview selectable and read-only.
                let mut sql = export.preview.sql.as_str();
                ui.add(egui::TextEdit::multiline(&mut sql)
                    .id_salt("sql-ddl")
                    .code_editor()
                    .desired_rows(16)
                    .desired_width(f32::INFINITY))
                    .labelled_by(label.id);
            });
            ui.horizontal(|ui| {
                save = ui.button("Save SQL…").clicked();
                if ui.button("Copy SQL").clicked() {
                    ctx.copy_text(export.preview.sql.clone());
                }
                close = ui.button("Close").clicked();
            });
        });
        if close || modal.should_close() {
            self.sql.export = None;
            return;
        }
        if previous_dialect != export.dialect || export.revision != self.history.revision() {
            match bp_sql::export_page(&self.doc, export.page, export.dialect) {
                Ok(preview) => {
                    export.preview = preview;
                    export.revision = self.history.revision();
                }
                Err(error) => {
                    self.error = Some(error);
                    return;
                }
            }
        }
        if save {
            let name = format!("{}-export.sql", self.file_stem());
            if let Some(mut path) = rfd::FileDialog::new()
                .add_filter("SQL schema", &["sql"])
                .set_file_name(name)
                .save_file()
            {
                if path.extension().is_none() {
                    path.set_extension("sql");
                }
                self.save_sql_to(&path);
            }
        }
    }
}

fn dialect_picker(ui: &mut egui::Ui, id: &str, dialect: &mut SqlDialect) {
    ui.horizontal(|ui| {
        ui.label("SQL dialect");
        egui::ComboBox::from_id_salt(id)
            .selected_text(dialect.label())
            .show_ui(ui, |ui| {
                for implemented in SUPPORTED_DIALECTS {
                    ui.selectable_value(dialect, *implemented, implemented.label());
                }
            });
    });
}

fn warnings(ui: &mut egui::Ui, warnings: &[Warning]) {
    if warnings.is_empty() {
        ui.label("No warnings.");
        return;
    }
    ui.label(egui::RichText::new(format!("{} warnings", warnings.len())).strong());
    egui::ScrollArea::vertical()
        .id_salt("sql-warnings")
        .max_height(140.0)
        .show(ui, |ui| {
            for warning in warnings {
                let message = if warning.line == 0 {
                    warning.message.clone()
                } else {
                    format!("Line {}: {}", warning.line, warning.message)
                };
                ui.label(message);
            }
        });
}

/// Includes symlink and hard-link aliases so an export preserves source scripts.
fn same_file(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    if let (Ok(a), Ok(b)) = (a.canonicalize(), b.canonicalize())
        && a == b
    {
        return true;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let (Ok(a), Ok(b)) = (a.metadata(), b.metadata()) {
            return a.dev() == b.dev() && a.ino() == b.ino();
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use bp_commands::{Command, LayerProp};
    use bp_model::ElementId;

    fn app() -> BlueprintApp {
        let mut app = BlueprintApp::new(&egui::Context::default(), None);
        app.choose_page_kind(DiagramKind::Erd);
        app
    }

    #[test]
    fn import_preserves_source_and_project_save_path_and_export_uses_separate_file() {
        let directory = std::env::temp_dir().join(format!("bp-sql-app-{}", ElementId::new()));
        std::fs::create_dir_all(&directory).unwrap();
        let source = directory.join("schema.sql");
        let project = directory.join("drawing.blueprint");
        let sql = "CREATE TABLE users (id integer PRIMARY KEY);";
        std::fs::write(&source, sql).unwrap();
        let mut app = app();
        app.path = Some(project.clone());
        app.open_sql_path(&source);
        assert_eq!(app.path, Some(project.clone()));
        app.preview_sql_import();
        assert!(app.apply_sql_import());
        app.open_sql_export();
        assert!(!app.save_sql_to(&source));
        assert_eq!(std::fs::read_to_string(&source).unwrap(), sql);
        app.error = None;
        assert!(!app.save_sql_to(&project));
        app.error = None;
        let output = directory.join("export.sql");
        assert!(app.save_sql_to(&output));
        assert!(
            std::fs::read_to_string(output)
                .unwrap()
                .contains("CREATE TABLE")
        );
        assert_eq!(app.path, Some(project));
        assert_eq!(std::fs::read_to_string(&source).unwrap(), sql);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn import_rejects_locked_layer_without_changing_document() {
        let mut app = app();
        app.apply(
            "Lock layer",
            [Command::SetLayer {
                id: app.layer,
                prop: LayerProp::Locked(true),
            }],
        );
        let before = app.doc.clone();
        app.paste_sql_schema();
        app.sql.import.as_mut().unwrap().source = "CREATE TABLE users (id integer);".into();
        app.preview_sql_import();
        assert!(!app.apply_sql_import());
        assert_eq!(app.doc, before);
        assert!(
            app.sql
                .import
                .as_ref()
                .unwrap()
                .error
                .as_ref()
                .unwrap()
                .contains("unlocked")
        );
    }

    #[cfg(unix)]
    #[test]
    fn source_aliases_cannot_be_overwritten_by_export() {
        let directory = std::env::temp_dir().join(format!("bp-sql-aliases-{}", ElementId::new()));
        std::fs::create_dir_all(&directory).unwrap();
        let source = directory.join("schema.sql");
        let sql = "CREATE TABLE users (id integer PRIMARY KEY);";
        std::fs::write(&source, sql).unwrap();
        let mut app = app();
        app.open_sql_path(&source);
        app.preview_sql_import();
        assert!(app.apply_sql_import());
        app.open_sql_export();
        let hardlink = directory.join("hardlink.sql");
        let symlink = directory.join("symlink.sql");
        std::fs::hard_link(&source, &hardlink).unwrap();
        std::os::unix::fs::symlink(&source, &symlink).unwrap();
        for alias in [&hardlink, &symlink] {
            assert!(!app.save_sql_to(alias));
            app.error = None;
        }
        assert_eq!(std::fs::read_to_string(&source).unwrap(), sql);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
