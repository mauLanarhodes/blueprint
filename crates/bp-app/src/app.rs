//! Application state, file actions, the scene cache and the frame layout.

use crate::canvas::Drag;
use crate::palette::{PaletteState, QuickInsert};
use bp_commands::{Command, History};
use bp_geom::Guide;
use bp_model::kurbo::Point;
use bp_model::{Document, ElementId, Layer, LayerId, OrderKey, PageId, ShapeRef};
use bp_render_egui::Viewport;
use bp_scene::{Scene, SceneCache};
use bp_shapes::Libraries;
use egui::{Key, ViewportCommand};
use egui_phosphor::regular as icon;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Tool {
    Select,
    Pan,
    Connector,
    Text,
    /// Click or drag on the canvas to insert this library shape.
    Shape(ShapeRef),
}

impl Tool {
    /// The tools on the toolbar, with their shortcut keys.
    pub fn toolbar() -> Vec<(Tool, &'static str, &'static str, Key)> {
        let shape = |library: &str, name: &str| Tool::Shape(ShapeRef::new(library, name));
        vec![
            (Tool::Select, icon::CURSOR, "Select", Key::V),
            (Tool::Pan, icon::HAND, "Pan", Key::H),
            (Tool::Connector, icon::FLOW_ARROW, "Connector", Key::C),
            (Tool::Text, icon::TEXT_T, "Text", Key::T),
            (
                shape("basic", "rectangle"),
                icon::SQUARE,
                "Rectangle",
                Key::R,
            ),
            (shape("basic", "ellipse"), icon::CIRCLE, "Ellipse", Key::O),
            (
                shape("flowchart", "decision"),
                icon::DIAMOND,
                "Decision",
                Key::D,
            ),
            (
                shape("basic", "sticky-note"),
                icon::NOTE,
                "Sticky note",
                Key::N,
            ),
        ]
    }
}

/// Something that would discard unsaved work, waiting for the user's answer.
#[derive(Clone, Debug, PartialEq)]
pub enum Pending {
    New,
    Open(Option<PathBuf>),
    Quit,
}

/// Text being edited in place on the canvas.
pub struct TextEditing {
    pub id: ElementId,
    pub text: String,
    pub focused_once: bool,
}

/// A page or layer being renamed inline.
#[derive(Clone, Debug, PartialEq)]
pub enum Renaming {
    Page(PageId, String),
    Layer(LayerId, String),
}

pub struct Settings {
    pub show_grid: bool,
    pub snap_to_grid: bool,
    pub snap_to_shapes: bool,
    /// Grid spacing in page units.
    pub grid: f64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            show_grid: true,
            snap_to_grid: true,
            snap_to_shapes: true,
            grid: 10.0,
        }
    }
}

pub struct BlueprintApp {
    pub doc: Document,
    pub history: History,
    pub path: Option<PathBuf>,
    saved_state: u64,
    pub page: PageId,
    /// Where new elements go.
    pub layer: LayerId,
    pub view: Viewport,
    views: HashMap<PageId, Viewport>,
    pub scene: Scene,
    cache: SceneCache,
    scene_key: Option<(u64, PageId)>,
    pub libraries: &'static Libraries,
    pub tool: Tool,
    /// Selected elements, in the order they were selected.
    pub selection: Vec<ElementId>,
    /// The group being edited from the inside (after a double-click), if any.
    pub scope: Option<ElementId>,
    pub drag: Drag,
    pub guides: Vec<Guide>,
    pub editing: Option<TextEditing>,
    pub canvas_rect: egui::Rect,
    /// The pointer position on the page, when it is over the canvas.
    pub pointer: Option<Point>,
    pub fit_requested: bool,
    pub settings: Settings,
    pub palette: PaletteState,
    pub quick_insert: Option<QuickInsert>,
    /// The last copied clip, for duplicate and when the system clipboard
    /// is unavailable.
    pub clip: Option<String>,
    pub renaming: Option<Renaming>,
    pub pending: Option<Pending>,
    pub error: Option<String>,
    pub status: String,
    allow_close: bool,
    title: String,
}

impl BlueprintApp {
    pub fn new(ctx: &egui::Context, file: Option<PathBuf>) -> Self {
        crate::theme::install(ctx);
        let mut doc = Document::new();
        let (page, layer) = first_page_and_layer(&mut doc);
        let mut app = Self {
            doc,
            history: History::new(),
            path: None,
            saved_state: 0,
            page,
            layer,
            view: Viewport::default(),
            views: HashMap::new(),
            scene: Scene::default(),
            cache: SceneCache::default(),
            scene_key: None,
            libraries: Libraries::builtin(),
            tool: Tool::Select,
            selection: Vec::new(),
            scope: None,
            drag: Drag::None,
            guides: Vec::new(),
            editing: None,
            canvas_rect: egui::Rect::NOTHING,
            pointer: None,
            fit_requested: false,
            settings: Settings::default(),
            palette: PaletteState::default(),
            quick_insert: None,
            clip: None,
            renaming: None,
            pending: None,
            error: None,
            status: "Ready".into(),
            allow_close: false,
            title: String::new(),
        };
        if let Some(path) = file {
            app.open_path(&path);
        }
        app
    }

    pub fn is_dirty(&self) -> bool {
        self.history.state_id() != self.saved_state
    }

    pub fn file_name(&self) -> String {
        self.path
            .as_deref()
            .and_then(Path::file_name)
            .map_or_else(|| "Untitled".into(), |n| n.to_string_lossy().into_owned())
    }

    /// Brings the scene up to date with the document (cheap when nothing
    /// changed; only changed elements are rebuilt otherwise).
    pub fn refresh_scene(&mut self) {
        let key = (self.history.revision(), self.page);
        if self.scene_key != Some(key) {
            // Dropping the old scene first lets the cache update the
            // spatial index it shares in place.
            self.scene = Scene::default();
            self.scene = self.cache.build(&self.doc, self.page, self.libraries);
            self.scene_key = Some(key);
        }
    }

    // ----- Editing -------------------------------------------------------

    /// Applies commands as one undo step (or into the open step),
    /// reporting failures in the status bar. Returns whether it worked.
    pub fn apply(&mut self, label: &str, commands: impl IntoIterator<Item = Command>) -> bool {
        let commands: Vec<Command> = commands.into_iter().collect();
        if commands.is_empty() {
            return false;
        }
        match self.history.apply(&mut self.doc, label, commands) {
            Ok(()) => true,
            Err(e) => {
                self.status = format!("{label} failed: {e}");
                false
            }
        }
    }

    /// Like [`Self::apply`] but merges rapid repeats (sliders, colour
    /// pickers, nudges) into one undo step.
    pub fn apply_merging(&mut self, label: &str, key: String, commands: Vec<Command>) {
        if commands.is_empty() {
            return;
        }
        if let Err(e) = self
            .history
            .apply_merging(&mut self.doc, label, key, commands)
        {
            self.status = format!("{label} failed: {e}");
        }
    }

    pub fn undo(&mut self) {
        self.editing = None;
        self.cancel_drag();
        if self.history.undo(&mut self.doc) {
            self.after_history_jump();
        }
    }

    pub fn redo(&mut self) {
        self.editing = None;
        self.cancel_drag();
        if self.history.redo(&mut self.doc) {
            self.after_history_jump();
        }
    }

    /// Abandons a drag in progress, undoing what it changed.
    pub fn cancel_drag(&mut self) {
        if std::mem::take(&mut self.drag).edits() {
            self.history.cancel(&mut self.doc);
        }
        self.guides.clear();
    }

    fn after_history_jump(&mut self) {
        // Undo can remove the page or layer we were on.
        if !self.doc.pages.contains_key(&self.page) {
            let page = self.doc.first_page().expect("documents keep a page");
            self.set_page(page);
        }
        if !self.doc.layers.contains_key(&self.layer) {
            self.layer = self.default_layer(self.page);
        }
        self.prune_selection();
    }

    // ----- Pages and layers ---------------------------------------------

    /// The topmost visible, unlocked layer of `page` (or its top layer).
    pub fn default_layer(&mut self, page: PageId) -> LayerId {
        let layers = self.doc.layers_of(page);
        if let Some(l) = layers.iter().rev().find(|l| l.visible && !l.locked) {
            return l.id;
        }
        if let Some(l) = layers.last() {
            return l.id;
        }
        // A page without layers (from an old or hand-edited file).
        let layer = Layer::new(page, "Layer 1", OrderKey::first());
        let id = layer.id;
        self.apply("Add layer", [Command::InsertLayer(Box::new(layer))]);
        id
    }

    pub fn set_page(&mut self, page: PageId) {
        if page == self.page && self.doc.pages.contains_key(&page) {
            return;
        }
        self.finish_text_edit(true);
        self.cancel_drag();
        self.views.insert(self.page, self.view);
        self.page = page;
        self.layer = self.default_layer(page);
        self.selection.clear();
        self.scope = None;
        match self.views.get(&page) {
            Some(view) => self.view = *view,
            None => self.fit_requested = true,
        }
    }

    // ----- Files ---------------------------------------------------------

    /// Runs `action` now, or asks to save first if there are unsaved changes.
    pub fn request(&mut self, action: Pending) {
        if self.is_dirty() {
            self.pending = Some(action);
        } else {
            self.perform(action);
        }
    }

    pub fn perform(&mut self, action: Pending) {
        match action {
            Pending::New => self.reset(Document::new(), None),
            Pending::Open(Some(path)) => self.open_path(&path),
            Pending::Open(None) => {
                if let Some(path) = file_dialog().pick_file() {
                    self.open_path(&path);
                }
            }
            Pending::Quit => self.allow_close = true,
        }
    }

    fn reset(&mut self, mut doc: Document, path: Option<PathBuf>) {
        let (page, layer) = first_page_and_layer(&mut doc);
        self.doc = doc;
        self.page = page;
        self.layer = layer;
        self.path = path;
        self.history.clear();
        self.saved_state = self.history.state_id();
        self.cache.clear();
        self.selection.clear();
        self.scope = None;
        self.editing = None;
        self.drag = Drag::None;
        self.guides.clear();
        self.views.clear();
        self.view = Viewport::default();
        self.fit_requested = true;
    }

    pub fn open_path(&mut self, path: &Path) {
        match bp_io::load(path) {
            Ok(doc) => {
                self.reset(doc, Some(path.to_owned()));
                self.status = format!("Opened {}", self.file_name());
            }
            Err(e) => self.error = Some(format!("Could not open {}:\n{e}", path.display())),
        }
    }

    /// Saves to the current path, or asks for one. Returns whether it saved.
    pub fn save(&mut self) -> bool {
        match self.path.clone() {
            Some(path) => self.save_to(path),
            None => self.save_as(),
        }
    }

    pub fn save_as(&mut self) -> bool {
        let name = format!("{}.blueprint", self.file_stem());
        match file_dialog().set_file_name(name).save_file() {
            Some(path) => self.save_to(bp_io::with_default_extension(path)),
            None => false,
        }
    }

    fn save_to(&mut self, path: PathBuf) -> bool {
        self.finish_text_edit(true);
        self.cancel_drag();
        self.history.commit();
        match bp_io::save(&self.doc, &path) {
            Ok(()) => {
                self.path = Some(path);
                self.status = format!("Saved {}", self.file_name());
                self.saved_state = self.history.state_id();
                true
            }
            Err(e) => {
                self.error = Some(format!("Could not save {}:\n{e}", path.display()));
                false
            }
        }
    }

    pub fn export_svg(&mut self) {
        self.finish_text_edit(true);
        let page_name = self
            .doc
            .pages
            .get(&self.page)
            .map(|p| p.name.clone())
            .unwrap_or_default();
        let multi = self.doc.pages.len() > 1;
        let name = if multi {
            format!("{} - {page_name}.svg", self.file_stem())
        } else {
            format!("{}.svg", self.file_stem())
        };
        let dialog = rfd::FileDialog::new()
            .add_filter("SVG image", &["svg"])
            .set_file_name(name);
        let Some(mut path) = dialog.save_file() else {
            return;
        };
        if path.extension().is_none() {
            path.set_extension("svg");
        }
        let options = bp_export::SvgOptions {
            embed_fonts: true,
            ..Default::default()
        };
        self.refresh_scene();
        let svg = bp_export::to_svg(&self.scene.list, &options);
        match std::fs::write(&path, svg) {
            Ok(()) => self.status = format!("Exported {}", path.display()),
            Err(e) => self.error = Some(format!("Could not export {}:\n{e}", path.display())),
        }
    }

    pub fn file_stem(&self) -> String {
        let name = self.file_name();
        let name = name.strip_suffix(".json").unwrap_or(&name);
        name.strip_suffix(".blueprint").unwrap_or(name).to_owned()
    }

    pub fn zoom_by(&mut self, factor: f32) {
        let center = self.canvas_rect.center();
        self.view.zoom_around(self.canvas_rect.min, center, factor);
    }

    // ----- Frame -----------------------------------------------------------

    fn handle_window_events(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) && self.is_dirty() && !self.allow_close {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.pending = Some(Pending::Quit);
        }
        if self.allow_close {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
        let dropped = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .find(|p| !p.as_os_str().is_empty())
        });
        if let Some(path) = dropped {
            self.request(Pending::Open(Some(path)));
        }
        let title = format!(
            "{}{} — Blueprint",
            self.file_name(),
            if self.is_dirty() { " •" } else { "" }
        );
        if title != self.title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    /// Lays out one frame. Separate from [`eframe::App`] so tests can run it.
    pub fn frame(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if !crate::theme::fonts_ready(&ctx) {
            // Only possible on the very first frame when the app is created
            // inside a running frame (tests, embedding).
            ctx.request_repaint();
            return;
        }
        self.handle_window_events(&ctx);
        if self.pending.is_none() && self.error.is_none() {
            self.handle_shortcuts(&ctx);
        }
        self.refresh_scene();

        egui::Panel::top("menu_bar").show(ui, |ui| self.menu_bar(ui));
        egui::Panel::top("toolbar").show(ui, |ui| self.toolbar(ui));
        egui::Panel::bottom("status_bar").show(ui, |ui| self.status_bar(ui));
        egui::Panel::bottom("page_tabs")
            .frame(
                egui::Frame::side_top_panel(ui.style()).inner_margin(egui::Margin::symmetric(8, 4)),
            )
            .show(ui, |ui| self.page_tabs(ui));
        egui::Panel::left("palette")
            .default_size(236.0)
            .size_range(180.0..=420.0)
            .show(ui, |ui| self.palette(ui));
        egui::Panel::right("inspector")
            .default_size(276.0)
            .size_range(220.0..=480.0)
            .show(ui, |ui| self.inspector(ui));
        egui::CentralPanel::no_frame().show(ui, |ui| self.canvas(ui));

        self.text_editor(&ctx);
        self.quick_insert_popup(&ctx);
        self.palette_drag_preview(&ctx);
        self.dialogs(&ctx);
    }
}

impl eframe::App for BlueprintApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.frame(ui);
    }
}

pub fn file_dialog() -> rfd::FileDialog {
    rfd::FileDialog::new()
        .add_filter("Blueprint project", &["blueprint", "json"])
        .add_filter("All files", &["*"])
}

/// The page and layer to draw on, adding a layer if the page has none.
fn first_page_and_layer(doc: &mut Document) -> (PageId, LayerId) {
    let page = doc.first_page().expect("validated documents have a page");
    if let Some(layer) = doc
        .layers_of(page)
        .iter()
        .rev()
        .find(|l| l.visible && !l.locked)
        .or(doc.layers_of(page).last())
    {
        return (page, layer.id);
    }
    let layer = Layer::new(page, "Layer 1", OrderKey::first());
    let id = layer.id;
    doc.layers.insert(id, layer);
    (page, id)
}
