//! Cached, searchable navigation over real pages, layers, groups and columns.
use crate::app::Renaming;
use crate::{BlueprintApp, InspectorTab};
use bp_commands::LayerProp;
use bp_model::{ColumnId, ElementId, LayerId, PageId, Parent, Tree};
use egui::{AtomExt, Button, Id, RichText, Ui};
use egui_phosphor::regular as icon;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Target {
    Layer(LayerId),
    Element(ElementId),
    Column(ElementId, ColumnId),
}
#[derive(Clone)]
struct Row {
    target: Target,
    name: String,
    glyph: &'static str,
    depth: usize,
    parents: Vec<Target>,
    children: bool,
    visible: bool,
    locked: bool,
}
#[derive(Default)]
pub(crate) struct NavigatorState {
    pub(crate) query: String,
    key: Option<(u64, PageId)>,
    rows: Arc<[Row]>,
    filtered: Arc<[usize]>,
    filter_key: Option<(String, usize)>,
    collapsed: HashSet<Target>,
    generation: usize,
    focus_rows: HashMap<Id, Target>,
    focus_requested: Option<Target>,
}

fn element_rows(
    app: &BlueprintApp,
    tree: &Tree<'_>,
    parent: Parent,
    depth: usize,
    parents: &[Target],
    visible: bool,
    rows: &mut Vec<Row>,
) {
    for &element in tree.children(parent) {
        let target = Target::Element(element.id);
        let children = !tree.children(Parent::Element(element.id)).is_empty();
        let columns = element.as_shape().and_then(|s| s.erd.as_ref());
        let name = element
            .text()
            .filter(|s| !s.trim().is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| {
                if element.is_group() {
                    "Group".into()
                } else if element.is_connector() {
                    "Connector".into()
                } else {
                    "Shape".into()
                }
            });
        rows.push(Row {
            target,
            name,
            glyph: if columns.is_some() {
                icon::TABLE
            } else if element.is_group() {
                icon::FOLDER
            } else if element.is_connector() {
                icon::FLOW_ARROW
            } else {
                icon::CUBE
            },
            depth,
            parents: parents.to_vec(),
            children: children || columns.is_some(),
            visible,
            locked: app.doc.is_locked(element.id),
        });
        let mut ancestors = parents.to_vec();
        ancestors.push(target);
        if let Some(table) = columns {
            for column in table.columns_sorted() {
                rows.push(Row {
                    target: Target::Column(element.id, column.id),
                    name: format!("{}  ·  {}", column.name, column.data_type),
                    glyph: if column.primary_key {
                        icon::KEY
                    } else {
                        icon::ROWS
                    },
                    depth: depth + 1,
                    parents: ancestors.clone(),
                    children: false,
                    visible,
                    locked: app.doc.is_locked(element.id),
                });
            }
        }
        element_rows(
            app,
            tree,
            Parent::Element(element.id),
            depth + 1,
            &ancestors,
            visible,
            rows,
        );
    }
}

impl BlueprintApp {
    pub(crate) fn navigator_has_focus(&self, ctx: &egui::Context) -> bool {
        ctx.memory(|memory| memory.focused())
            .is_some_and(|id| self.workspace.navigator.focus_rows.contains_key(&id))
    }
    fn refresh_navigator(&mut self) {
        let key = (self.history.revision(), self.page);
        let stable_during_drag = self.drag.is_active()
            && self
                .workspace
                .navigator
                .key
                .is_some_and(|(_, page)| page == self.page);
        if self.workspace.navigator.key != Some(key) && !stable_during_drag {
            let tree = self.doc.tree();
            let mut rows = Vec::new();
            for layer in self.doc.layers_of(self.page).into_iter().rev() {
                let target = Target::Layer(layer.id);
                rows.push(Row {
                    target,
                    name: layer.name.clone(),
                    glyph: icon::STACK,
                    depth: 0,
                    parents: Vec::new(),
                    children: !tree.children(Parent::Layer(layer.id)).is_empty(),
                    visible: layer.visible,
                    locked: layer.locked,
                });
                element_rows(
                    self,
                    &tree,
                    Parent::Layer(layer.id),
                    1,
                    &[target],
                    layer.visible,
                    &mut rows,
                );
            }
            let nav = &mut self.workspace.navigator;
            nav.rows = rows.into();
            nav.key = Some(key);
            nav.filter_key = None;
        }
        let nav = &mut self.workspace.navigator;
        let filter_key = (nav.query.to_lowercase(), nav.generation);
        if nav.filter_key.as_ref() != Some(&filter_key) {
            let query = filter_key.0.trim();
            let mut included = HashSet::new();
            if !query.is_empty() {
                for row in nav
                    .rows
                    .iter()
                    .filter(|row| row.name.to_lowercase().contains(query))
                {
                    included.insert(row.target);
                    included.extend(row.parents.iter().copied());
                }
            }
            nav.filtered = nav
                .rows
                .iter()
                .enumerate()
                .filter(|(_, row)| {
                    if query.is_empty() {
                        !row.parents.iter().any(|p| nav.collapsed.contains(p))
                    } else {
                        included.contains(&row.target)
                    }
                })
                .map(|(i, _)| i)
                .collect::<Vec<_>>()
                .into();
            nav.filter_key = Some(filter_key);
        }
    }

    fn navigator_target(&mut self, target: Target) {
        match target {
            Target::Layer(id) => {
                self.layer = id;
                self.workspace.selected_column = None;
            }
            Target::Element(id) => self.select_navigation_element(id, None),
            Target::Column(id, column) => self.select_navigation_element(id, Some(column)),
        }
    }

    pub fn select_navigation_element(&mut self, id: ElementId, column: Option<ColumnId>) {
        if !self.doc.elements.contains_key(&id) || self.doc.page_of(id) != Some(self.page) {
            return;
        }
        if !self
            .doc
            .layer_of(id)
            .and_then(|layer| self.doc.layers.get(&layer))
            .is_some_and(|layer| layer.visible)
        {
            self.status = "Show the layer before selecting its contents".into();
            return;
        }
        self.finish_inline_edits();
        self.cancel_drag();
        self.scope = self
            .doc
            .ancestors(id)
            .into_iter()
            .find(|parent| self.doc.elements.get(parent).is_some_and(|e| e.is_group()));
        self.select_only(id);
        if let Some(column) = column.filter(|column| {
            self.doc
                .elements
                .get(&id)
                .and_then(|e| e.as_shape())
                .and_then(|s| s.erd.as_ref())
                .is_some_and(|table| table.column(*column).is_some())
        }) {
            self.workspace.selected_column = Some((id, column));
            self.workspace.inspector_tab = InspectorTab::Columns;
        }
    }

    pub fn frame_element(&mut self, id: ElementId) {
        self.refresh_scene();
        if let Some(bounds) = self.scene.bounds_of(id) {
            self.view.fit(self.canvas_rect, bounds, 80.0);
        }
    }

    pub(crate) fn handle_navigation_keys(&mut self, ctx: &egui::Context) {
        if self.preferences.left_tab != crate::LeftTab::Navigator {
            return;
        }
        let Some(focused) = ctx.memory(|m| m.focused()) else {
            return;
        };
        let Some(target) = self.workspace.navigator.focus_rows.get(&focused).copied() else {
            return;
        };
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
            self.navigator_target(target);
        }
        let step = if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown))
        {
            1
        } else if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp)) {
            -1
        } else {
            0
        };
        if step != 0 {
            let nav = &self.workspace.navigator;
            let Some(index) = nav
                .filtered
                .iter()
                .position(|&i| nav.rows[i].target == target)
            else {
                return;
            };
            let mut next = index as isize + step;
            while next >= 0 && next < nav.filtered.len() as isize {
                let row = &nav.rows[nav.filtered[next as usize]];
                if row.visible || matches!(row.target, Target::Layer(_)) {
                    break;
                }
                next += step;
            }
            if next < 0 || next >= nav.filtered.len() as isize {
                return;
            }
            let next = next as usize;
            let target = nav.rows[nav.filtered[next]].target;
            self.navigator_target(target);
            self.workspace.navigator.focus_requested = Some(target);
        }
        let collapse =
            ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft));
        let expand = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight));
        if collapse || expand {
            let nav = &mut self.workspace.navigator;
            if collapse {
                nav.collapsed.insert(target);
            } else {
                nav.collapsed.remove(&target);
            }
            nav.generation += 1;
        }
    }

    pub(crate) fn navigator(&mut self, ui: &mut Ui) {
        let search = ui.add(
            crate::widgets::text_field(&mut self.workspace.navigator.query)
                .hint_text("Search pages and objects")
                .desired_width(f32::INFINITY),
        );
        search.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Search pages and objects")
        });
        crate::widgets::focus_outline(ui, &search);
        ui.horizontal(|ui| {
            crate::widgets::panel_title(ui, "Pages");
            if crate::widgets::icon_button(ui, icon::PLUS, "Add page").clicked() {
                self.request_add_page();
            }
        });
        let pages: Vec<_> = self
            .doc
            .pages_sorted()
            .iter()
            .map(|p| (p.id, p.name.clone()))
            .collect();
        egui::ScrollArea::vertical()
            .id_salt("navigator_pages")
            .max_height(112.0)
            .show(ui, |ui| {
                for (index, (id, name)) in pages.iter().enumerate() {
                    if !self.workspace.navigator.query.is_empty()
                        && !name
                            .to_lowercase()
                            .contains(&self.workspace.navigator.query.to_lowercase())
                    {
                        continue;
                    }
                    if let Some(Renaming::Page(renaming, text)) = &mut self.renaming
                        && renaming == id
                    {
                        let response =
                            ui.add(crate::widgets::text_field(text).desired_width(f32::INFINITY));
                        response.request_focus();
                        let cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));
                        let done =
                            response.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter));
                        if cancel {
                            self.renaming = None;
                        } else if done {
                            let text = text.clone();
                            self.renaming = None;
                            self.rename_page(*id, text);
                        }
                        continue;
                    }
                    let response = ui.add_sized(
                        [ui.available_width(), 28.0],
                        Button::new((
                            crate::theme::icon(icon::FILE),
                            name.as_str()
                                .atom_shrink(true)
                                .atom_grow(true)
                                .atom_align(egui::Align2::LEFT_CENTER),
                        ))
                        .selected(*id == self.page)
                        .truncate(),
                    );
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::SelectableLabel, true, name)
                    });
                    crate::widgets::focus_outline(ui, &response);
                    if response.clicked() {
                        self.set_page(*id);
                    }
                    if response.double_clicked() {
                        self.renaming = Some(Renaming::Page(*id, name.clone()));
                    }
                    response.context_menu(|ui| {
                        if ui.button("Rename page").clicked() {
                            self.renaming = Some(Renaming::Page(*id, name.clone()));
                            ui.close();
                        }
                        if ui.button("Duplicate page").clicked() {
                            self.duplicate_page(*id);
                            ui.close();
                        }
                        if ui
                            .add_enabled(index > 0, Button::new("Move page up"))
                            .clicked()
                        {
                            self.move_page(*id, -1);
                            ui.close();
                        }
                        if ui
                            .add_enabled(index + 1 < pages.len(), Button::new("Move page down"))
                            .clicked()
                        {
                            self.move_page(*id, 1);
                            ui.close();
                        }
                        if ui
                            .add_enabled(pages.len() > 1, Button::new("Delete page"))
                            .clicked()
                        {
                            self.delete_page(*id);
                            ui.close();
                        }
                    });
                }
            });
        ui.separator();
        ui.horizontal(|ui| {
            crate::widgets::panel_title(ui, "Layers and objects");
            if crate::widgets::icon_button(ui, icon::PLUS, "Add layer").clicked() {
                self.add_layer();
            }
        });
        self.refresh_navigator();
        let rows = self.workspace.navigator.rows.clone();
        let filtered = self.workspace.navigator.filtered.clone();
        if filtered.is_empty() {
            ui.label(RichText::new("No objects match").weak());
        }
        let focus = self.workspace.navigator.focus_requested;
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt("navigator_objects")
            .auto_shrink(false);
        if let Some(target) = focus
            && let Some(index) = filtered.iter().position(|&i| rows[i].target == target)
        {
            scroll = scroll.vertical_scroll_offset(index as f32 * 34.0);
        }
        self.workspace.navigator.focus_rows.clear();
        scroll.show_rows(ui, 28.0, filtered.len(), |ui, range| {
            for index in range {
                let row = &rows[filtered[index]];
                ui.push_id(row.target, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        ui.add_space((row.depth as f32 * 12.0).min(48.0));
                        if row.children {
                            let collapsed = self.workspace.navigator.collapsed.contains(&row.target);
                            if crate::widgets::icon_button(ui, if collapsed { icon::CARET_RIGHT } else { icon::CARET_DOWN }, if collapsed { "Expand object" } else { "Collapse object" }).clicked() {
                                if collapsed { self.workspace.navigator.collapsed.remove(&row.target); } else { self.workspace.navigator.collapsed.insert(row.target); }
                                self.workspace.navigator.generation += 1;
                            }
                        } else { ui.add_space(28.0); }
                        let selected = match row.target { Target::Layer(id) => self.layer == id, Target::Element(id) => self.is_selected(id) && self.workspace.selected_column.is_none(), Target::Column(id, column) => self.workspace.selected_column == Some((id, column)) };
                        // The icon buttons include glyph padding and border strokes.
                        // Their 28-point minimum is not their rendered width. Reserve
                        // the complete trailing control area, including row gaps.
                        let extra = if matches!(row.target, Target::Layer(_)) { 80.0 } else { 0.0 };
                        let name_width = (ui.available_width() - extra).max(40.0);
                        let renaming = match (&self.renaming, row.target) {
                            (Some(Renaming::Layer(id, text)), Target::Layer(target)) if *id == target => Some((target, text.clone())),
                            _ => None,
                        };
                        let response = if let Some((id, mut text)) = renaming.clone() {
                            let response = ui.add(crate::widgets::text_field(&mut text).hint_text("Layer name").desired_width(name_width));
                            response.request_focus();
                            if ui.input(|i| i.key_pressed(egui::Key::Escape)) { self.renaming = None; }
                            else if response.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter)) { self.renaming = None; self.rename_layer(id, text); }
                            else { self.renaming = Some(Renaming::Layer(id, text)); }
                            response
                        } else {
                            let response = ui.add_enabled_ui(row.visible || matches!(row.target, Target::Layer(_)), |ui| {
                                // The name must shrink, not the leading icon atom.
                                ui.add_sized([name_width, 28.0], Button::new((crate::theme::icon(row.glyph), row.name.as_str().atom_shrink(true).atom_grow(true).atom_align(egui::Align2::LEFT_CENTER))).selected(selected).truncate())
                            }).inner;
                            response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::SelectableLabel, row.visible || matches!(row.target, Target::Layer(_)), &row.name));
                            self.workspace.navigator.focus_rows.insert(response.id, row.target);
                            if focus == Some(row.target) { response.request_focus(); self.workspace.navigator.focus_requested = None; }
                            if response.has_focus() {
                                ui.memory_mut(|memory| memory.set_focus_lock_filter(response.id, egui::EventFilter {
                                    vertical_arrows: true, horizontal_arrows: true, ..Default::default()
                                }));
                            }
                            if response.clicked() { self.navigator_target(row.target); }
                            if response.double_clicked() { match row.target { Target::Element(id) | Target::Column(id, _) => self.frame_element(id), Target::Layer(id) => self.renaming = Some(Renaming::Layer(id, row.name.clone())) } }
                            response
                        };
                        crate::widgets::focus_outline(ui, &response);
                        if let Target::Layer(id) = row.target {
                            if crate::widgets::icon_button(ui, if row.visible { icon::EYE } else { icon::EYE_SLASH }, if row.visible { "Hide layer" } else { "Show layer" }).clicked() { self.set_layer_flag(id, LayerProp::Visible(!row.visible)); }
                            if crate::widgets::icon_button(ui, if row.locked { icon::LOCK } else { icon::LOCK_OPEN }, if row.locked { "Unlock layer" } else { "Lock layer" }).clicked() { self.set_layer_flag(id, LayerProp::Locked(!row.locked)); }
                        }
                        response.on_hover_text(&row.name).context_menu(|ui| {
                            match row.target {
                                Target::Layer(id) => {
                                    if ui.button("Rename layer").clicked() { self.renaming = Some(Renaming::Layer(id, row.name.clone())); ui.close(); }
                                    if ui.button("Move layer up").clicked() { self.move_layer(id, true); ui.close(); }
                                    if ui.button("Move layer down").clicked() { self.move_layer(id, false); ui.close(); }
                                    if ui.add_enabled(self.doc.layers_of(self.page).len() > 1, Button::new("Delete layer")).clicked() { self.delete_layer(id); ui.close(); }
                                    let movable = self.selection.iter().any(|selected| self.doc.elements.get(selected).is_some_and(|element| matches!(element.parent, Parent::Layer(source) if source != id) && !self.doc.is_locked(*selected)));
                                    if ui.add_enabled(!row.locked && row.visible && movable, Button::new("Move selection to layer")).clicked() { self.move_selection_to_layer(id); ui.close(); }
                                }
                                Target::Element(id) | Target::Column(id, _) => {
                                    if ui.add_enabled(row.visible, Button::new("Frame selected object")).clicked() { self.frame_element(id); ui.close(); }
                                }
                            }
                        });
                    });
                });
            }
        });
    }
}
