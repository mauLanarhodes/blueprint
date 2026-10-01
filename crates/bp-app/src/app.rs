//! Application state, file actions and the per-frame layout.

use bp_commands::{Command, History};
use bp_model::kurbo::{Point, Rect};
use bp_model::{Document, Element, ElementId, LayerId, OrderKey, PageId, ShapeKind};
use bp_render_egui::Viewport;
use egui::{Key, KeyboardShortcut, Modifiers, ViewportCommand};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Select,
    Pan,
    Shape(ShapeKind),
}

impl Tool {
    pub const ALL: [(Tool, &'static str, Key); 7] = [
        (Tool::Select, "Select", Key::V),
        (Tool::Pan, "Pan", Key::H),
        (Tool::Shape(ShapeKind::Rectangle), "Rectangle", Key::R),
        (Tool::Shape(ShapeKind::RoundedRectangle), "Rounded", Key::U),
        (Tool::Shape(ShapeKind::Ellipse), "Ellipse", Key::O),
        (Tool::Shape(ShapeKind::Diamond), "Diamond", Key::D),
        (Tool::Shape(ShapeKind::Text), "Text", Key::T),
    ];
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

pub struct BlueprintApp {
    pub doc: Document,
    pub history: History,
    pub path: Option<PathBuf>,
    saved_state: u64,
    pub page: PageId,
    pub layer: LayerId,
    pub tool: Tool,
    pub selection: Option<ElementId>,
    pub view: Viewport,
    pub drag: crate::canvas::Drag,
    pub editing: Option<TextEditing>,
    pub canvas_rect: egui::Rect,
    pub fit_requested: bool,
    pub show_grid: bool,
    pub pending: Option<Pending>,
    pub error: Option<String>,
    pub status: String,
    allow_close: bool,
    title: String,
}

pub mod shortcuts {
    use super::*;
    const fn cmd(key: Key) -> KeyboardShortcut {
        KeyboardShortcut::new(Modifiers::COMMAND, key)
    }
    const fn cmd_shift(key: Key) -> KeyboardShortcut {
        KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), key)
    }
    pub const NEW: KeyboardShortcut = cmd(Key::N);
    pub const OPEN: KeyboardShortcut = cmd(Key::O);
    pub const SAVE: KeyboardShortcut = cmd(Key::S);
    pub const SAVE_AS: KeyboardShortcut = cmd_shift(Key::S);
    pub const EXPORT_SVG: KeyboardShortcut = cmd(Key::E);
    pub const QUIT: KeyboardShortcut = cmd(Key::Q);
    pub const UNDO: KeyboardShortcut = cmd(Key::Z);
    pub const REDO: KeyboardShortcut = cmd_shift(Key::Z);
    pub const REDO_ALT: KeyboardShortcut = cmd(Key::Y);
    pub const ZOOM_IN: KeyboardShortcut = cmd(Key::Equals);
    pub const ZOOM_OUT: KeyboardShortcut = cmd(Key::Minus);
    pub const ZOOM_100: KeyboardShortcut = cmd(Key::Num0);
    pub const ZOOM_FIT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::SHIFT, Key::Num1);
    pub const FRONT: KeyboardShortcut = cmd_shift(Key::CloseBracket);
    pub const BACK: KeyboardShortcut = cmd_shift(Key::OpenBracket);
}

impl BlueprintApp {
    pub fn new(cc: &eframe::CreationContext<'_>, file: Option<PathBuf>) -> Self {
        cc.egui_ctx.set_theme(egui::Theme::Light);
        let mut doc = Document::new();
        let (page, layer) = first_page_and_layer(&mut doc);
        let mut app = Self {
            doc,
            history: History::new(),
            path: None,
            saved_state: 0,
            page,
            layer,
            tool: Tool::Select,
            selection: None,
            view: Viewport::default(),
            drag: crate::canvas::Drag::None,
            editing: None,
            canvas_rect: egui::Rect::NOTHING,
            fit_requested: false,
            show_grid: true,
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

    // ----- Editing -------------------------------------------------------

    /// Applies commands as one undo step, reporting failures in the status bar.
    pub fn apply(&mut self, label: &str, commands: impl IntoIterator<Item = Command>) {
        if let Err(e) = self.history.apply(&mut self.doc, label, commands) {
            self.status = format!("{label} failed: {e}");
        }
    }

    /// Like [`Self::apply`] but merges rapid repeats (sliders, colour pickers).
    pub fn apply_merging(&mut self, label: &str, key: String, command: Command) {
        if let Err(e) = self
            .history
            .apply_merging(&mut self.doc, label, key, [command])
        {
            self.status = format!("{label} failed: {e}");
        }
    }

    pub fn create_shape(&mut self, kind: ShapeKind, bounds: Rect) {
        let order = self.doc.next_order_key(self.layer);
        let mut element = Element::new(kind, self.layer, order, bounds);
        if kind == ShapeKind::Text {
            element.text = "Text".into();
        }
        let id = element.id;
        self.apply(
            &format!("Add {}", kind.label().to_lowercase()),
            [Command::Insert(Box::new(element))],
        );
        self.selection = Some(id);
        self.tool = Tool::Select;
        if kind == ShapeKind::Text {
            self.start_text_edit(id);
        }
    }

    /// Default-sized shape centred on `at`, for a click without a drag.
    pub fn create_shape_at(&mut self, kind: ShapeKind, at: Point) {
        let size = match kind {
            ShapeKind::Text => (120.0, 32.0),
            ShapeKind::Diamond => (120.0, 90.0),
            _ => (140.0, 70.0),
        };
        self.create_shape(kind, Rect::from_center_size(at, size));
    }

    pub fn delete_selection(&mut self) {
        if let Some(id) = self.selection.take() {
            self.editing = None;
            self.apply("Delete", [Command::Remove(id)]);
        }
    }

    pub fn bring_to_front(&mut self) {
        let Some(id) = self.selection else { return };
        let Some(layer) = self.doc.elements.get(&id).map(|e| e.layer) else {
            return;
        };
        let order = self.doc.next_order_key(layer);
        self.apply("Bring to front", [Command::SetOrder { id, order }]);
    }

    pub fn send_to_back(&mut self) {
        let Some(id) = self.selection else { return };
        let Some(layer) = self.doc.elements.get(&id).map(|e| e.layer) else {
            return;
        };
        let lowest = self
            .doc
            .elements
            .values()
            .filter(|e| e.layer == layer)
            .map(|e| &e.order)
            .min()
            .cloned();
        if let Some(lowest) = lowest {
            let order = OrderKey::before(&lowest);
            self.apply("Send to back", [Command::SetOrder { id, order }]);
        }
    }

    pub fn start_text_edit(&mut self, id: ElementId) {
        if let Some(el) = self.doc.elements.get(&id) {
            self.selection = Some(id);
            self.editing = Some(TextEditing {
                id,
                text: el.text.clone(),
                focused_once: false,
            });
        }
    }

    pub fn finish_text_edit(&mut self, keep: bool) {
        let Some(edit) = self.editing.take() else {
            return;
        };
        let changed = self
            .doc
            .elements
            .get(&edit.id)
            .is_some_and(|el| el.text != edit.text);
        if keep && changed {
            self.apply(
                "Edit text",
                [Command::SetText {
                    id: edit.id,
                    text: edit.text,
                }],
            );
        }
    }

    pub fn undo(&mut self) {
        self.editing = None;
        if self.history.undo(&mut self.doc) {
            self.after_history_jump();
        }
    }

    pub fn redo(&mut self) {
        self.editing = None;
        if self.history.redo(&mut self.doc) {
            self.after_history_jump();
        }
    }

    fn after_history_jump(&mut self) {
        if self
            .selection
            .is_some_and(|id| !self.doc.elements.contains_key(&id))
        {
            self.selection = None;
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
        self.selection = None;
        self.editing = None;
        self.drag = crate::canvas::Drag::None;
        self.view = Viewport::default();
        self.fit_requested = true;
    }

    pub fn open_path(&mut self, path: &Path) {
        match bp_io::load(path) {
            Ok(doc) => {
                self.reset(doc, Some(path.to_owned()));
                self.status = format!("Opened {}", path.display());
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
        self.history.commit();
        match bp_io::save(&self.doc, &path) {
            Ok(()) => {
                self.status = format!("Saved {}", path.display());
                self.path = Some(path);
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
        let dialog = rfd::FileDialog::new()
            .add_filter("SVG image", &["svg"])
            .set_file_name(format!("{}.svg", self.file_stem()));
        let Some(mut path) = dialog.save_file() else {
            return;
        };
        if path.extension().is_none() {
            path.set_extension("svg");
        }
        let svg = bp_export::page_to_svg(&self.doc, self.page, &Default::default());
        match std::fs::write(&path, svg) {
            Ok(()) => self.status = format!("Exported {}", path.display()),
            Err(e) => self.error = Some(format!("Could not export {}:\n{e}", path.display())),
        }
    }

    fn file_stem(&self) -> String {
        let name = self.file_name();
        let name = name.strip_suffix(".json").unwrap_or(&name);
        name.strip_suffix(".blueprint").unwrap_or(name).to_owned()
    }

    // ----- Frame -----------------------------------------------------------

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        use shortcuts::*;
        let typing = ctx.text_edit_focused();
        let pressed = |s: KeyboardShortcut| ctx.input_mut(|i| i.consume_shortcut(&s));

        // Shift variants first: Ctrl+Z also matches Ctrl+Shift+Z.
        if pressed(SAVE_AS) {
            self.save_as();
        } else if pressed(SAVE) {
            self.save();
        } else if pressed(NEW) {
            self.request(Pending::New);
        } else if pressed(OPEN) {
            self.request(Pending::Open(None));
        } else if pressed(EXPORT_SVG) {
            self.export_svg();
        } else if pressed(QUIT) {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
        if typing {
            return;
        }
        if pressed(REDO) || pressed(REDO_ALT) {
            self.redo();
        } else if pressed(UNDO) {
            self.undo();
        } else if pressed(FRONT) {
            self.bring_to_front();
        } else if pressed(BACK) {
            self.send_to_back();
        } else if pressed(ZOOM_IN) {
            self.zoom_by(1.25);
        } else if pressed(ZOOM_OUT) {
            self.zoom_by(0.8);
        } else if pressed(ZOOM_100) {
            self.zoom_by(1.0 / self.view.zoom);
        } else if pressed(ZOOM_FIT) {
            self.fit_requested = true;
        }

        let (keys, modifiers) = ctx.input(|i| {
            let keys: Vec<Key> = [
                Key::Delete,
                Key::Backspace,
                Key::Escape,
                Key::Enter,
                Key::ArrowLeft,
                Key::ArrowRight,
                Key::ArrowUp,
                Key::ArrowDown,
            ]
            .into_iter()
            .chain(Tool::ALL.iter().map(|t| t.2))
            .filter(|k| i.key_pressed(*k))
            .collect();
            (keys, i.modifiers)
        });
        for key in keys {
            match key {
                Key::Delete | Key::Backspace => self.delete_selection(),
                Key::Escape => {
                    // Cancel a drag in progress, otherwise clear the selection.
                    if std::mem::replace(&mut self.drag, crate::canvas::Drag::None).is_edit() {
                        self.history.cancel(&mut self.doc);
                    } else {
                        self.selection = None;
                    }
                    self.tool = Tool::Select;
                }
                Key::Enter => {
                    if let Some(id) = self.selection {
                        self.start_text_edit(id);
                    }
                }
                Key::ArrowLeft | Key::ArrowRight | Key::ArrowUp | Key::ArrowDown => {
                    let step = if modifiers.shift { 10.0 } else { 1.0 };
                    let (dx, dy) = match key {
                        Key::ArrowLeft => (-step, 0.0),
                        Key::ArrowRight => (step, 0.0),
                        Key::ArrowUp => (0.0, -step),
                        _ => (0.0, step),
                    };
                    self.nudge(dx, dy);
                }
                tool_key if modifiers.is_none() => {
                    if let Some((tool, _, _)) = Tool::ALL.iter().find(|t| t.2 == tool_key) {
                        self.tool = *tool;
                    }
                }
                _ => {}
            }
        }
    }

    fn nudge(&mut self, dx: f64, dy: f64) {
        let Some(id) = self.selection else { return };
        let Some(bounds) = self.doc.elements.get(&id).map(|e| e.bounds) else {
            return;
        };
        let bounds = bounds + bp_model::kurbo::Vec2::new(dx, dy);
        self.apply_merging(
            "Nudge",
            format!("nudge:{id}"),
            Command::SetBounds { id, bounds },
        );
    }

    pub fn zoom_by(&mut self, factor: f32) {
        let center = self.canvas_rect.center();
        self.view.zoom_around(self.canvas_rect.min, center, factor);
    }

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
}

impl eframe::App for BlueprintApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_window_events(&ctx);
        if self.pending.is_none() && self.error.is_none() {
            self.handle_shortcuts(&ctx);
        }

        egui::Panel::top("menu_bar").show(ui, |ui| self.menu_bar(ui));
        egui::Panel::bottom("status_bar").show(ui, |ui| self.status_bar(ui));
        egui::Panel::left("tools")
            .resizable(false)
            .exact_size(132.0)
            .show(ui, |ui| self.tool_bar(ui));
        egui::Panel::right("inspector")
            .default_size(250.0)
            .show(ui, |ui| self.inspector(ui));
        egui::CentralPanel::no_frame().show(ui, |ui| self.canvas(ui));

        self.text_editor(&ctx);
        self.dialogs(&ctx);
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
    if let Some(layer) = doc.layers_of(page).last() {
        return (page, layer.id);
    }
    let layer = bp_model::Layer {
        id: LayerId::new(),
        page,
        name: "Layer 1".into(),
        order: OrderKey::first(),
        visible: true,
        locked: false,
    };
    let id = layer.id;
    doc.layers.insert(id, layer);
    (page, id)
}