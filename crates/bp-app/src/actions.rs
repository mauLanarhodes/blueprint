//! Editing actions behind the menus, shortcuts and inspector.

use crate::app::{BlueprintApp, Pending, Tool};
use bp_commands::edit::{self, Align, Clip, Reorder};
use bp_commands::{Command, LayerProp, PageProp, Prop};
use bp_model::kurbo::{Point, Rect, Vec2};
use bp_model::{
    DiagramKind, Element, ElementId, ElementKind, Endpoint, Layer, LayerId, OrderKey, Page, PageId,
    Parent, ShapeRef,
};
use egui::{Key, KeyboardShortcut, Modifiers};

/// The key under which copied elements travel on the system clipboard.
const CLIP_KEY: &str = "blueprint-clip";

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
    pub const SELECT_ALL: KeyboardShortcut = cmd(Key::A);
    pub const DUPLICATE: KeyboardShortcut = cmd(Key::D);
    pub const GROUP: KeyboardShortcut = cmd(Key::G);
    pub const UNGROUP: KeyboardShortcut = cmd_shift(Key::G);
    pub const LOCK: KeyboardShortcut = cmd(Key::L);
    pub const FRONT: KeyboardShortcut = cmd_shift(Key::CloseBracket);
    pub const BACK: KeyboardShortcut = cmd_shift(Key::OpenBracket);
    pub const FORWARD: KeyboardShortcut = cmd(Key::CloseBracket);
    pub const BACKWARD: KeyboardShortcut = cmd(Key::OpenBracket);
    pub const ZOOM_IN: KeyboardShortcut = cmd(Key::Equals);
    pub const ZOOM_OUT: KeyboardShortcut = cmd(Key::Minus);
    pub const ZOOM_100: KeyboardShortcut = cmd(Key::Num0);
    pub const ZOOM_FIT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::SHIFT, Key::Num1);
    pub const NEXT_PAGE: KeyboardShortcut = cmd(Key::PageDown);
    pub const PREVIOUS_PAGE: KeyboardShortcut = cmd(Key::PageUp);
}

impl BlueprintApp {
    /// Where new elements go: inside the group being edited, or on the
    /// active layer.
    pub fn insert_parent(&self) -> Parent {
        self.scope
            .filter(|s| self.doc.elements.contains_key(s))
            .map_or(Parent::Layer(self.layer), Parent::Element)
    }

    /// Whether new elements can go on the active layer; explains why not.
    fn can_insert(&mut self) -> bool {
        match self.doc.layers.get(&self.layer) {
            Some(l) if !l.visible => {
                self.status = format!("“{}” is hidden; show it or pick another layer", l.name);
                false
            }
            Some(l) if l.locked => {
                self.status = format!("“{}” is locked; unlock it or pick another layer", l.name);
                false
            }
            Some(_) => true,
            None => false,
        }
    }

    pub fn insert_shape(&mut self, shape: ShapeRef, bounds: Rect) -> Option<ElementId> {
        if !self.can_insert() {
            return None;
        }
        let asset = self.cloud_icon(&shape).cloned();
        if shape.is_cloud() && asset.is_none() {
            self.status = "Import this cloud icon pack before inserting its icons".into();
            return None;
        }
        let name = asset.as_ref().map_or_else(
            || self.libraries.resolve(&shape).name.clone(),
            |asset| asset.name.clone(),
        );
        let parent = self.insert_parent();
        let order = self.doc.next_order_key(parent);
        let mut el = Element::shape(shape.clone(), parent, order, bounds);
        let mut commands = Vec::new();
        if let Some(asset) = asset {
            el.as_shape_mut().unwrap().text = asset.name.clone();
            if !self.doc.icons.contains_key(&shape) {
                commands.push(Command::InsertIcon(Box::new(asset)));
            }
        }
        let id = el.id;
        commands.push(Command::Insert(Box::new(el)));
        if self.apply(&format!("Add {name}"), commands) {
            self.select_only(id);
            self.palette.note_used(&shape);
            Some(id)
        } else {
            None
        }
    }

    /// Inserts `shape` at its default size, centred on `center` (snapped
    /// to the grid when snapping is on).
    pub fn insert_shape_at(&mut self, shape: ShapeRef, center: Point) -> Option<ElementId> {
        let size = if shape.is_cloud() {
            (64.0, 64.0)
        } else {
            self.libraries.resolve(&shape).default_size
        };
        let mut bounds = Rect::from_center_size(center, size);
        if self.settings.snap_to_grid {
            let g = self.settings.grid;
            let origin = Point::new((bounds.x0 / g).round() * g, (bounds.y0 / g).round() * g);
            bounds = bounds + (origin - bounds.origin());
        }
        self.insert_shape(shape, bounds)
    }

    pub fn insert_connector(&mut self, source: Endpoint, target: Endpoint) -> Option<ElementId> {
        if !self.can_insert() {
            return None;
        }
        let parent = self.insert_parent();
        let order = self.doc.next_order_key(parent);
        let relationship = self.table_relationship(&source, &target);
        let mut el = Element::connector(source, target, parent, order);
        let id = el.id;
        let mut commands = Vec::new();
        if let Some((start, end, table, column)) = relationship {
            if let Some(c) = el.as_connector_mut() {
                c.start_marker = start;
                c.end_marker = end;
                let identifying = self
                    .doc
                    .elements
                    .get(&table)
                    .and_then(Element::as_shape)
                    .and_then(|s| s.erd.as_ref())
                    .and_then(|t| t.column(column))
                    .is_some_and(|c| c.primary_key);
                c.style.dash = Some(if identifying {
                    bp_model::Dash::Solid
                } else {
                    bp_model::Dash::Dashed
                });
            }
            commands.push(Command::SetColumn {
                id: table,
                column,
                prop: bp_commands::ColumnProp::ForeignKey(true),
            });
        }
        if let Some(connector) = el.as_connector_mut() {
            self.configure_connection(connector);
        }
        commands.push(Command::Insert(Box::new(el)));
        self.apply("Add connector", commands).then(|| {
            self.select_only(id);
            id
        })
    }

    pub fn delete_selection(&mut self) {
        let ids = self.editable_selection();
        if ids.is_empty() {
            if !self.selection.is_empty() {
                self.status = "Locked elements can't be deleted; unlock them first".into();
            }
            return;
        }
        self.editing = None;
        let commands = edit::remove(&self.doc, &ids);
        if self.apply("Delete", commands) {
            self.selection.clear();
        }
    }

    pub fn select_all(&mut self) {
        self.selection = self.all_selectable();
    }

    // ----- Clipboard ------------------------------------------------------

    /// The selection as clipboard text.
    fn copy_text(&mut self) -> Option<String> {
        if self.selection.is_empty() {
            return None;
        }
        self.refresh_scene();
        let scene = &self.scene;
        let clip = Clip::copy(&self.doc, &self.selection, |connector, source| {
            let g = scene.connector(connector)?;
            if source {
                g.points.first().copied()
            } else {
                g.points.last().copied()
            }
        });
        Some(clip_to_text(&clip))
    }

    pub fn copy(&mut self, ctx: &egui::Context) {
        if let Some(text) = self.copy_text() {
            ctx.copy_text(text.clone());
            self.clip = Some(text);
            self.status = format!("Copied {} element(s)", self.selection.len());
        }
    }

    pub fn cut(&mut self, ctx: &egui::Context) {
        self.copy(ctx);
        self.delete_selection();
    }

    /// Pastes Blueprint elements, or plain text as a text box.
    pub fn paste_text(&mut self, text: &str) {
        match clip_from_text(text) {
            Some(clip) => self.paste_clip(&clip),
            None if !text.trim().is_empty() => {
                let center = self.pointer.unwrap_or_else(|| self.view_center());
                self.history.begin("Paste text");
                if let Some(id) = self.insert_shape_at(ShapeRef::new("basic", "text"), center) {
                    self.apply(
                        "Paste text",
                        [Command::Set {
                            id,
                            prop: Prop::Text(text.trim().to_owned()),
                        }],
                    );
                    self.fit_text_shape(id);
                }
                self.history.commit();
            }
            None => {}
        }
    }

    fn paste_clip(&mut self, clip: &Clip) {
        if clip.is_empty() || !self.can_insert() {
            return;
        }
        for icon in clip.icons.values() {
            if let Err(reason) = bp_icons::validate_svg(&icon.svg) {
                self.status = format!("Cannot paste {}: {reason}", icon.name);
                return;
            }
        }
        let bounds = clip
            .elements
            .iter()
            .filter_map(|e| e.as_shape().map(|s| s.bounds))
            .reduce(|a, b| a.union(b));
        let offset = match bounds {
            // Paste beside the original while it is in view, otherwise in
            // the middle of the view.
            Some(b) if self.visible_page_rect().intersect(b).area() > 0.0 => {
                Vec2::new(self.settings.grid * 2.0, self.settings.grid * 2.0)
            }
            Some(b) => self.view_center() - b.center(),
            None => Vec2::new(20.0, 20.0),
        };
        let parent = self.insert_parent();
        let (roots, commands) = clip.paste(&self.doc, parent, offset);
        if self.apply("Paste", commands) {
            self.selection = roots;
            // The next paste goes beside this one.
            let shifted = shift_clip(clip, offset);
            self.clip = Some(clip_to_text(&shifted));
        }
    }

    pub fn duplicate(&mut self) {
        if let Some(text) = self.copy_text()
            && let Some(clip) = clip_from_text(&text)
        {
            self.paste_clip(&clip);
        }
    }

    // ----- Arrange ---------------------------------------------------------

    pub fn group_selection(&mut self) {
        let ids = self.editable_selection();
        if ids.len() < 2 {
            return;
        }
        if let Some((group, commands)) = edit::group(&self.doc, &ids)
            && self.apply("Group", commands)
        {
            self.select_only(group);
        }
    }

    pub fn ungroup_selection(&mut self) {
        let ids = self.editable_selection();
        let (freed, commands) = edit::ungroup(&self.doc, &ids);
        if self.apply("Ungroup", commands) {
            self.selection = freed;
        }
    }

    pub fn reorder(&mut self, how: Reorder) {
        let ids = self.editable_selection();
        let label = match how {
            Reorder::Front => "Bring to front",
            Reorder::Forward => "Bring forward",
            Reorder::Backward => "Send backward",
            Reorder::Back => "Send to back",
        };
        let commands = edit::reorder(&self.doc, &ids, how);
        self.apply(label, commands);
    }

    pub fn align(&mut self, how: Align) {
        let ids = self.editable_selection();
        let commands = edit::align(&self.doc, &ids, how);
        self.apply("Align", commands);
    }

    pub fn distribute(&mut self, horizontal: bool) {
        let ids = self.editable_selection();
        let commands = edit::distribute(&self.doc, &ids, horizontal);
        self.apply("Distribute", commands);
    }

    /// Locks the selection, or unlocks it if everything is locked already.
    pub fn toggle_lock(&mut self) {
        let lock = !self
            .selection
            .iter()
            .all(|id| self.doc.elements.get(id).is_some_and(|e| e.locked));
        let commands: Vec<Command> = self
            .selection
            .iter()
            .map(|&id| Command::Set {
                id,
                prop: Prop::Locked(lock),
            })
            .collect();
        self.apply(if lock { "Lock" } else { "Unlock" }, commands);
    }

    pub fn nudge(&mut self, dx: f64, dy: f64) {
        let ids = self.editable_selection();
        let commands = edit::translate(&self.doc, &ids, Vec2::new(dx, dy));
        self.apply_merging("Nudge", "nudge".into(), commands);
    }

    /// Sets a property on every selected element that has it.
    pub fn set_on_selection(
        &mut self,
        label: &str,
        merge: bool,
        make: impl Fn(&Element) -> Option<Prop>,
    ) {
        let commands: Vec<Command> = self
            .editable_selection()
            .iter()
            .filter_map(|id| {
                let el = self.doc.elements.get(id)?;
                Some(Command::Set {
                    id: *id,
                    prop: make(el)?,
                })
            })
            .collect();
        if merge {
            let key = format!("{label}:{:?}", self.selection);
            self.apply_merging(label, key, commands);
        } else {
            self.apply(label, commands);
        }
    }

    // ----- Text ------------------------------------------------------------

    /// Commits editors before a save, close or document replacement.
    pub(crate) fn finish_inline_edits(&mut self) {
        self.finish_text_edit(true);
        match self.renaming.take() {
            Some(crate::app::Renaming::Page(id, name)) => self.rename_page(id, name),
            Some(crate::app::Renaming::Layer(id, name)) => self.rename_layer(id, name),
            None => {}
        }
    }

    pub fn start_text_edit(&mut self, id: ElementId) {
        if self.doc.is_locked(id) {
            self.status = "Locked elements can't be edited".into();
            return;
        }
        if let Some(text) = self.doc.elements.get(&id).and_then(|e| e.text()) {
            self.selection = vec![id];
            self.editing = Some(crate::app::TextEditing {
                id,
                text: text.to_owned(),
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
            .and_then(|e| e.text())
            .is_some_and(|t| t != edit.text);
        if keep && changed {
            // The text and the box fitted to it are one undo step.
            self.history.begin("Edit text");
            self.apply(
                "Edit text",
                [Command::Set {
                    id: edit.id,
                    prop: Prop::Text(edit.text),
                }],
            );
            self.fit_text_shape(edit.id);
            self.history.commit();
        }
    }

    /// Grows or shrinks a text box's height to fit its text.
    pub fn fit_text_shape(&mut self, id: ElementId) {
        let Some(shape) = self.doc.elements.get(&id).and_then(Element::as_shape) else {
            return;
        };
        if shape.shape != ShapeRef::new("basic", "text") {
            return;
        }
        let def = self.libraries.resolve(&shape.shape);
        let style = shape.style.resolve(&def.default_style());
        let width = def.text_box(shape.bounds).width() - 2.0 * bp_scene::PADDING_X;
        let face = bp_text::Face::new(style.bold, style.italic);
        let layout = bp_text::layout(&shape.text, face, style.font_size, Some(width.max(1.0)));
        let height = (layout.height + 2.0 * bp_scene::PADDING_Y).max(style.font_size * 1.6);
        let b = shape.bounds;
        if (b.height() - height).abs() > 0.5 {
            let bounds = Rect::new(b.x0, b.y0, b.x1, b.y0 + height);
            self.apply(
                "Fit text",
                [Command::Set {
                    id,
                    prop: Prop::Bounds(bounds),
                }],
            );
        }
    }

    // ----- Pages -----------------------------------------------------------

    pub fn add_page(&mut self) {
        self.request_add_page();
    }

    pub fn add_page_with_kind(&mut self, kind: DiagramKind) {
        let pages = self.doc.pages_sorted();
        let current = pages.iter().position(|p| p.id == self.page).unwrap_or(0);
        let below = pages[current].order.clone();
        let above = pages.get(current + 1).map(|p| p.order.clone());
        let mut n = pages.len() + 1;
        while pages.iter().any(|p| p.name == format!("Page {n}")) {
            n += 1;
        }
        let mut page = Page::new(
            format!("Page {n}"),
            OrderKey::between(Some(&below), above.as_ref()),
        );
        page.diagram_kind = Some(kind);
        let layer = Layer::new(page.id, "Layer 1", OrderKey::first());
        let id = page.id;
        if self.apply(
            "Add page",
            [
                Command::InsertPage(Box::new(page)),
                Command::InsertLayer(Box::new(layer)),
            ],
        ) {
            self.set_page(id);
        }
    }

    pub fn delete_page(&mut self, page: PageId) {
        if self.doc.pages.len() < 2 {
            self.status = "A document needs at least one page".into();
            return;
        }
        if page == self.page {
            let pages = self.doc.pages_sorted();
            let i = pages.iter().position(|p| p.id == page).unwrap_or(0);
            let next = pages
                .get(i + 1)
                .or(pages.get(i.wrapping_sub(1)))
                .map(|p| p.id);
            if let Some(next) = next {
                self.set_page(next);
            }
        }
        let commands = edit::remove_page(&self.doc, page);
        self.apply("Delete page", commands);
    }

    pub fn duplicate_page(&mut self, source: PageId) {
        let Some(original) = self.doc.pages.get(&source).cloned() else {
            return;
        };
        let pages = self.doc.pages_sorted();
        let i = pages.iter().position(|p| p.id == source).unwrap_or(0);
        let above = pages.get(i + 1).map(|p| p.order.clone());
        let mut page = Page::new(
            format!("{} copy", original.name),
            OrderKey::between(Some(&original.order), above.as_ref()),
        );
        page.background = original.background;
        page.diagram_kind = self.page_kind_for(source);
        let new_page = page.id;
        let mut commands = vec![Command::InsertPage(Box::new(page))];
        let mut layers = std::collections::HashMap::new();
        for layer in self.doc.layers_of(source) {
            let mut copy = Layer::new(new_page, layer.name.clone(), layer.order.clone());
            copy.visible = layer.visible;
            copy.locked = layer.locked;
            let new_layer = copy.id;
            layers.insert(layer.id, new_layer);
            commands.push(Command::InsertLayer(Box::new(copy)));
        }
        // Remap the whole page together: a relationship can span layers,
        // including hidden layers that have no resolved scene geometry.
        let elements: Vec<&Element> = self
            .doc
            .elements
            .values()
            .filter(|e| self.doc.page_of(e.id) == Some(source))
            .collect();
        let fresh: std::collections::HashMap<ElementId, ElementId> =
            elements.iter().map(|e| (e.id, ElementId::new())).collect();
        let mut copies: Vec<(usize, Element)> = elements
            .into_iter()
            .map(|original| {
                let mut copy = original.clone();
                copy.id = fresh[&original.id];
                copy.parent = match original.parent {
                    Parent::Layer(id) => Parent::Layer(layers[&id]),
                    Parent::Element(id) => Parent::Element(fresh[&id]),
                };
                if let ElementKind::Connector(c) = &mut copy.kind {
                    for end in [&mut c.source, &mut c.target] {
                        if let Endpoint::Glued { element, .. } = end
                            && let Some(id) = fresh.get(element)
                        {
                            *element = *id;
                        }
                    }
                }
                (self.doc.ancestors(original.id).len(), copy)
            })
            .collect();
        // Insert ancestors and shapes before dependent children/connectors.
        copies.sort_by_key(|(depth, e)| (e.is_connector(), *depth));
        commands.extend(
            copies
                .into_iter()
                .map(|(_, e)| Command::Insert(Box::new(e))),
        );
        if self.apply("Duplicate page", commands) {
            self.set_page(new_page);
        }
    }

    /// Moves `page` one place left (`-1`) or right (`1`) in the tabs.
    pub fn move_page(&mut self, page: PageId, step: isize) {
        let pages = self.doc.pages_sorted();
        let Some(i) = pages.iter().position(|p| p.id == page) else {
            return;
        };
        let order = match step {
            -1 if i > 0 => {
                let below = (i >= 2).then(|| pages[i - 2].order.clone());
                OrderKey::between(below.as_ref(), Some(&pages[i - 1].order))
            }
            1 if i + 1 < pages.len() => {
                let above = pages.get(i + 2).map(|p| p.order.clone());
                OrderKey::between(Some(&pages[i + 1].order), above.as_ref())
            }
            _ => return,
        };
        self.apply(
            "Move page",
            [Command::SetPage {
                id: page,
                prop: PageProp::Order(order),
            }],
        );
    }

    pub fn rename_page(&mut self, page: PageId, name: String) {
        let name = name.trim().to_owned();
        if !name.is_empty() && self.doc.pages.get(&page).is_some_and(|p| p.name != name) {
            self.apply(
                "Rename page",
                [Command::SetPage {
                    id: page,
                    prop: PageProp::Name(name),
                }],
            );
        }
    }

    pub fn step_page(&mut self, step: isize) {
        let pages: Vec<PageId> = self.doc.pages_sorted().iter().map(|p| p.id).collect();
        if let Some(i) = pages.iter().position(|p| *p == self.page) {
            let next = (i as isize + step).rem_euclid(pages.len() as isize) as usize;
            self.set_page(pages[next]);
        }
    }

    // ----- Layers ----------------------------------------------------------

    pub fn add_layer(&mut self) {
        let layers = self.doc.layers_of(self.page);
        let i = layers.iter().position(|l| l.id == self.layer);
        let below = i.map(|i| layers[i].order.clone());
        let above = i.and_then(|i| layers.get(i + 1)).map(|l| l.order.clone());
        let mut n = layers.len() + 1;
        while layers.iter().any(|l| l.name == format!("Layer {n}")) {
            n += 1;
        }
        let layer = Layer::new(
            self.page,
            format!("Layer {n}"),
            OrderKey::between(below.as_ref(), above.as_ref()),
        );
        let id = layer.id;
        if self.apply("Add layer", [Command::InsertLayer(Box::new(layer))]) {
            self.layer = id;
        }
    }

    pub fn delete_layer(&mut self, layer: LayerId) {
        if self.doc.layers_of(self.page).len() < 2 {
            self.status = "A page needs at least one layer".into();
            return;
        }
        let ids: Vec<ElementId> = self
            .doc
            .children(Parent::Layer(layer))
            .iter()
            .map(|e| e.id)
            .collect();
        let mut commands = edit::remove(&self.doc, &ids);
        commands.push(Command::RemoveLayer(layer));
        if self.apply("Delete layer", commands) && self.layer == layer {
            self.layer = self.default_layer(self.page);
        }
        self.prune_selection();
    }

    /// Moves `layer` up (towards the front) or down.
    pub fn move_layer(&mut self, layer: LayerId, up: bool) {
        let layers = self.doc.layers_of(self.page);
        let Some(i) = layers.iter().position(|l| l.id == layer) else {
            return;
        };
        let order = if up && i + 1 < layers.len() {
            let above = layers.get(i + 2).map(|l| l.order.clone());
            OrderKey::between(Some(&layers[i + 1].order), above.as_ref())
        } else if !up && i > 0 {
            let below = (i >= 2).then(|| layers[i - 2].order.clone());
            OrderKey::between(below.as_ref(), Some(&layers[i - 1].order))
        } else {
            return;
        };
        self.apply(
            "Move layer",
            [Command::SetLayer {
                id: layer,
                prop: LayerProp::Order(order),
            }],
        );
    }

    pub fn set_layer_flag(&mut self, layer: LayerId, prop: LayerProp) {
        let label = match prop {
            LayerProp::Visible(true) => "Show layer",
            LayerProp::Visible(false) => "Hide layer",
            LayerProp::Locked(true) => "Lock layer",
            LayerProp::Locked(false) => "Unlock layer",
            _ => "Change layer",
        };
        self.apply(label, [Command::SetLayer { id: layer, prop }]);
        self.prune_selection();
    }

    pub fn rename_layer(&mut self, layer: LayerId, name: String) {
        let name = name.trim().to_owned();
        if !name.is_empty() && self.doc.layers.get(&layer).is_some_and(|l| l.name != name) {
            self.apply(
                "Rename layer",
                [Command::SetLayer {
                    id: layer,
                    prop: LayerProp::Name(name),
                }],
            );
        }
    }

    /// Moves the selected top-level elements onto `layer`, on top.
    pub fn move_selection_to_layer(&mut self, layer: LayerId) {
        let target = Parent::Layer(layer);
        let mut order = self.doc.next_order_key(target);
        let mut commands = Vec::new();
        for id in edit::top_level(&self.doc, &self.editable_selection()) {
            if matches!(self.doc.elements[&id].parent, Parent::Layer(l) if l != layer) {
                commands.push(Command::Set {
                    id,
                    prop: Prop::Parent(target),
                });
                commands.push(Command::Set {
                    id,
                    prop: Prop::Order(order.clone()),
                });
                order = OrderKey::after(&order);
            }
        }
        if self.apply("Move to layer", commands) {
            self.layer = layer;
        }
    }

    // ----- Geometry helpers ---------------------------------------------------

    /// The part of the page visible in the canvas.
    pub fn visible_page_rect(&self) -> Rect {
        self.view
            .screen_to_page_rect(self.canvas_rect.min, self.canvas_rect)
    }

    pub fn view_center(&self) -> Point {
        self.visible_page_rect().center()
    }

    // ----- Keyboard ------------------------------------------------------------

    pub fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        use shortcuts::*;
        let typing = ctx.egui_wants_keyboard_input()
            || self.editing.is_some()
            || self.quick_insert.is_some();
        let pressed = |s: KeyboardShortcut| ctx.input_mut(|i| i.consume_shortcut(&s));

        // Shift variants first: Ctrl+S also matches Ctrl+Shift+S.
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
            self.request(Pending::Quit);
        }
        if typing {
            return;
        }

        for event in ctx.input(|i| i.events.clone()) {
            match event {
                egui::Event::Copy => self.copy(ctx),
                egui::Event::Cut => self.cut(ctx),
                egui::Event::Paste(text) => self.paste_text(&text),
                _ => {}
            }
        }

        if pressed(REDO) || pressed(REDO_ALT) {
            self.redo();
        } else if pressed(UNDO) {
            self.undo();
        } else if pressed(SELECT_ALL) {
            self.select_all();
        } else if pressed(DUPLICATE) {
            self.duplicate();
        } else if pressed(UNGROUP) {
            self.ungroup_selection();
        } else if pressed(GROUP) {
            self.group_selection();
        } else if pressed(LOCK) {
            self.toggle_lock();
        } else if pressed(FRONT) {
            self.reorder(Reorder::Front);
        } else if pressed(BACK) {
            self.reorder(Reorder::Back);
        } else if pressed(FORWARD) {
            self.reorder(Reorder::Forward);
        } else if pressed(BACKWARD) {
            self.reorder(Reorder::Backward);
        } else if pressed(ZOOM_IN) {
            self.zoom_by(1.25);
        } else if pressed(ZOOM_OUT) {
            self.zoom_by(0.8);
        } else if pressed(ZOOM_100) {
            self.zoom_by(1.0 / self.view.zoom);
        } else if pressed(ZOOM_FIT) {
            self.fit_requested = true;
        } else if pressed(NEXT_PAGE) {
            self.step_page(1);
        } else if pressed(PREVIOUS_PAGE) {
            self.step_page(-1);
        }

        let toolbar = self.available_tools();
        let (keys, modifiers, slash) = ctx.input(|i| {
            let keys: Vec<Key> = [
                Key::Delete,
                Key::Backspace,
                Key::Escape,
                Key::Enter,
                Key::F2,
                Key::ArrowLeft,
                Key::ArrowRight,
                Key::ArrowUp,
                Key::ArrowDown,
            ]
            .into_iter()
            .chain(toolbar.iter().map(|t| t.3))
            .filter(|k| i.key_pressed(*k))
            .collect();
            let slash = i
                .events
                .iter()
                .any(|e| matches!(e, egui::Event::Text(t) if t == "/"));
            (keys, i.modifiers, slash)
        });
        if slash {
            self.open_quick_insert();
        }
        for key in keys {
            match key {
                Key::Delete | Key::Backspace => self.delete_selection(),
                Key::Escape => self.escape(),
                Key::Enter | Key::F2 => {
                    if let [id] = self.selection[..] {
                        self.start_text_edit(id);
                    }
                }
                Key::ArrowLeft | Key::ArrowRight | Key::ArrowUp | Key::ArrowDown => {
                    let step = if modifiers.shift {
                        self.settings.grid
                    } else {
                        1.0
                    };
                    let (dx, dy) = match key {
                        Key::ArrowLeft => (-step, 0.0),
                        Key::ArrowRight => (step, 0.0),
                        Key::ArrowUp => (0.0, -step),
                        _ => (0.0, step),
                    };
                    self.nudge(dx, dy);
                }
                tool_key if modifiers.is_none() => {
                    if let Some((tool, ..)) = toolbar.iter().find(|t| t.3 == tool_key) {
                        self.tool = tool.clone();
                    }
                }
                Key::C if modifiers == Modifiers::SHIFT => self.cycle_erd_connection(),
                _ => {}
            }
        }
    }

    /// Escape backs out one level: a drag, the tool, the selection, then
    /// the group being edited.
    pub fn escape(&mut self) {
        if self.drag.is_active() {
            self.cancel_drag();
        } else if self.tool != Tool::Select {
            self.tool = Tool::Select;
        } else if !self.selection.is_empty() {
            self.selection.clear();
        } else {
            self.scope = None;
        }
    }
}

fn clip_to_text(clip: &Clip) -> String {
    serde_json::json!({ CLIP_KEY: 1, "elements": clip.elements, "icons": clip.icons }).to_string()
}

fn clip_from_text(text: &str) -> Option<Clip> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    value.get(CLIP_KEY)?;
    let elements = serde_json::from_value(value.get("elements")?.clone()).ok()?;
    let icons = value
        .get("icons")
        .map(|v| serde_json::from_value(v.clone()))
        .transpose()
        .ok()?
        .unwrap_or_default();
    Some(Clip { elements, icons })
}

/// `clip` moved by `offset`, so repeated pastes step across the page.
fn shift_clip(clip: &Clip, offset: Vec2) -> Clip {
    let mut out = clip.clone();
    for el in &mut out.elements {
        match &mut el.kind {
            ElementKind::Shape(s) => s.bounds = s.bounds + offset,
            ElementKind::Connector(c) => {
                for end in [&mut c.source, &mut c.target] {
                    if let Endpoint::Free(p) = end {
                        *p += offset;
                    }
                }
                for p in &mut c.waypoints {
                    *p += offset;
                }
            }
            ElementKind::Group => {}
        }
    }
    out
}
