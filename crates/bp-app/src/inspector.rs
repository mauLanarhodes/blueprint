//! The inspector (right panel): properties of the selection, or of the
//! page when nothing is selected.

use crate::app::BlueprintApp;
use crate::inspector_sections::InspectorTab;
use crate::theme::labelled;
use bp_commands::edit::{Align, Reorder};
use bp_commands::{Command, PageProp, Prop};
use bp_model::kurbo::Rect;
use bp_model::{
    Color, Dash, DiagramKind, Element, ElementKind, Marker, Paint, Routing, Style, StyleValues,
    TextAlign, VerticalAlign,
};
use bp_shapes::Outline;
use egui::{Button, DragValue, RichText, Ui};
use egui_phosphor::regular as icon;

impl BlueprintApp {
    pub fn inspector(&mut self, ui: &mut Ui) {
        let selected: Vec<Element> = self
            .selection
            .iter()
            .filter_map(|id| self.doc.elements.get(id).cloned())
            .collect();
        let kind = self.page_kind();
        let table = selected
            .first()
            .is_some_and(|el| el.as_shape().is_some_and(|shape| shape.erd.is_some()));
        let tabs: &[InspectorTab] = if selected.len() > 1 {
            &[InspectorTab::Design, InspectorTab::Arrange]
        } else if table || (selected.is_empty() && kind == Some(DiagramKind::Erd)) {
            &[
                InspectorTab::Table,
                InspectorTab::Columns,
                InspectorTab::Appearance,
            ]
        } else if kind == Some(DiagramKind::Cloud)
            || (kind == Some(DiagramKind::Erd)
                && selected
                    .first()
                    .is_some_and(|el| el.as_connector().is_some()))
        {
            &[InspectorTab::Configuration, InspectorTab::Appearance]
        } else {
            &[InspectorTab::Design, InspectorTab::Arrange]
        };
        self.inspector_tabs(ui, tabs);
        egui::ScrollArea::vertical()
            .id_salt("context-inspector")
            .auto_shrink(false)
            .show(ui, |ui| {
                ui.add_space(4.0);
                match selected.as_slice() {
                    [] => self.page_inspector(ui),
                    [one] => {
                        ui.add_enabled_ui(!self.doc.is_locked(one.id), |ui| match &one.kind {
                            ElementKind::Shape(_) => self.shape_inspector(ui, one),
                            ElementKind::Connector(_) => self.connector_inspector(ui, one),
                            ElementKind::Group => self.group_inspector(ui, one),
                        });
                        if one.locked && ui.button("Unlock selected element").clicked() {
                            self.toggle_lock();
                        }
                    }
                    many => self.multi_inspector(ui, many),
                }
            });
    }

    fn page_inspector(&mut self, ui: &mut Ui) {
        let Some(page) = self.doc.pages.get(&self.page).cloned() else {
            return;
        };
        ui.heading("Page");
        if self.workspace.inspector_tab == InspectorTab::Columns {
            ui.label(RichText::new("Select a table to edit its columns.").weak());
            return;
        }
        ui.add_space(4.0);
        let appearance = self.workspace.inspector_tab == InspectorTab::Appearance;
        egui::Grid::new("page").num_columns(2).show(ui, |ui| {
            if !appearance {
                ui.label("Name");
                let mut name = page.name.clone();
                let edit =
                    ui.add(crate::widgets::text_field(&mut name).desired_width(f32::INFINITY));
                if edit.changed() && !name.trim().is_empty() {
                    self.apply_merging(
                        "Rename page",
                        format!("page-name:{}", page.id),
                        vec![Command::SetPage {
                            id: page.id,
                            prop: PageProp::Name(name),
                        }],
                    );
                }
                ui.end_row();
                ui.label("Diagram type");
                let current_kind = self.page_kind();
                let mut chosen_kind = current_kind;
                egui::ComboBox::from_id_salt("page-diagram-kind")
                    .selected_text(current_kind.map_or("Choose type…", DiagramKind::label))
                    .show_ui(ui, |ui| {
                        for kind in [DiagramKind::Erd, DiagramKind::Flowchart, DiagramKind::Cloud] {
                            ui.selectable_value(&mut chosen_kind, Some(kind), kind.label());
                        }
                    });
                if chosen_kind != current_kind
                    && let Some(kind) = chosen_kind
                {
                    self.set_page_kind(kind);
                }
                ui.end_row();
            }
            if appearance || self.workspace.inspector_tab == InspectorTab::Design {
                ui.label("Background");
                let mut bg = page.background;
                if color_button(ui, &mut bg) {
                    self.apply_merging(
                        "Page background",
                        format!("page-bg:{}", page.id),
                        vec![Command::SetPage {
                            id: page.id,
                            prop: PageProp::Background(bg),
                        }],
                    );
                }
                ui.end_row();
            }
        });
        if self.page_kind() == Some(DiagramKind::Cloud) && !appearance {
            ui.add_space(10.0);
            section(ui, "AWS deployment target");
            self.deployment_target(ui);
        }
        ui.add_space(10.0);
        section(ui, "Grid and snapping");
        ui.checkbox(&mut self.settings.show_grid, "Show grid");
        ui.checkbox(&mut self.settings.snap_to_grid, "Snap to grid");
        ui.checkbox(&mut self.settings.snap_to_shapes, "Smart guides");
        ui.horizontal(|ui| {
            ui.label("Grid size");
            ui.add(
                DragValue::new(&mut self.settings.grid)
                    .range(2.0..=200.0)
                    .speed(0.5),
            );
        });
        ui.label(
            RichText::new("Hold Alt while dragging to place freely.")
                .small()
                .weak(),
        );
        if appearance {
            return;
        }
        ui.add_space(10.0);
        section(ui, "Shortcuts");
        let erd = self.page_kind() == Some(DiagramKind::Erd);
        let shape_shortcuts = if self.page_kind() == Some(DiagramKind::Flowchart) {
            ("R O D N", "Rectangle, ellipse, decision, note")
        } else {
            ("R O N", "Rectangle, ellipse, note")
        };
        egui::Grid::new("shortcuts")
            .num_columns(2)
            .striped(true)
            .show(ui, |ui| {
                for (keys, action) in [
                    ("V H C T", "Select, pan, connector, text"),
                    shape_shortcuts,
                    ("Shift+C", "Cycle ERD relationships"),
                    ("/", "Insert a shape by name"),
                    ("Drag a port", "Draw a connector"),
                    ("Double-click", "Edit text / enter group"),
                    ("Shift-click", "Add to selection"),
                    ("Ctrl+G", "Group (Shift: ungroup)"),
                    ("Ctrl+D", "Duplicate"),
                    ("Ctrl+L", "Lock / unlock"),
                    ("Ctrl+] / [", "Forward / backward"),
                    ("Space + drag", "Pan"),
                    ("Ctrl + scroll", "Zoom"),
                    ("Shift+1", "Zoom to fit"),
                    ("Arrows", "Nudge (Shift: grid)"),
                    ("Esc", "Cancel / deselect / leave group"),
                ] {
                    if keys == "Shift+C" && !erd {
                        continue;
                    }
                    ui.label(RichText::new(keys).monospace().small());
                    ui.label(RichText::new(action).small());
                    ui.end_row();
                }
            });
    }

    fn shape_inspector(&mut self, ui: &mut Ui, el: &Element) {
        let Some(shape) = el.as_shape() else { return };
        let def = self.libraries.resolve(&shape.shape);
        let cloud_icon = self.doc.icons.get(&shape.shape);
        let title = match shape.network_container {
            Some(container) => container.label(),
            None => cloud_icon.map_or(def.name.as_str(), |icon| icon.name.as_str()),
        };
        ui.heading(title);
        if shape.network_container.is_none() && shape.shape.is_cloud() {
            ui.label(
                RichText::new(if shape.shape.library() == "aws" {
                    "AWS architecture icon"
                } else {
                    "Azure architecture icon"
                })
                .small()
                .weak(),
            );
        }
        if self.doc.is_locked(el.id) {
            ui.label(RichText::new("Locked").weak());
        }
        if self.page_kind() == Some(DiagramKind::Cloud) {
            ui.label(
                RichText::new(
                    shape
                        .infrastructure
                        .as_ref()
                        .map_or("Diagram only", bp_model::AwsResource::label),
                )
                .small()
                .strong(),
            );
        }
        ui.add_space(6.0);
        match self.workspace.inspector_tab {
            InspectorTab::Table if shape.erd.is_some() => self.erd_table_inspector(ui, el),
            InspectorTab::Columns if shape.erd.is_some() => self.erd_columns_inspector(ui, el),
            InspectorTab::Configuration => {
                self.shape_text_inspector(ui, el);
                self.resource_inspector(ui, el);
            }
            InspectorTab::Arrange => {
                self.position_size_section(ui, el);
                self.arrange_section(ui);
            }
            InspectorTab::Appearance => {
                self.position_size_section(ui, el);
                self.shape_style_inspector(ui, el);
            }
            _ => {
                self.shape_text_inspector(ui, el);
                self.shape_style_inspector(ui, el);
            }
        }
    }

    fn shape_text_inspector(&mut self, ui: &mut Ui, el: &Element) {
        let Some(shape) = el.as_shape() else { return };
        if self.editing.is_some() {
            return;
        }
        section(
            ui,
            if self.page_kind() == Some(DiagramKind::Cloud) {
                "Name"
            } else {
                "Text"
            },
        );
        let mut text = shape.text.clone();
        let label = ui.label(if self.page_kind() == Some(DiagramKind::Cloud) {
            "Label"
        } else {
            "Content"
        });
        let response = ui
            .add(
                egui::TextEdit::multiline(&mut text)
                    .desired_rows(2)
                    .desired_width(f32::INFINITY),
            )
            .labelled_by(label.id);
        if response.changed() {
            let id = el.id;
            self.apply_merging(
                "Edit text",
                format!("text:{id}"),
                vec![Command::Set {
                    id,
                    prop: Prop::Text(text),
                }],
            );
        }
        ui.add_space(6.0);
    }

    fn shape_style_inspector(&mut self, ui: &mut Ui, el: &Element) {
        let Some(shape) = el.as_shape() else { return };
        if shape.network_container.is_some() {
            ui.label(RichText::new("Boundary size is independent of its contents. Header and boundary colors follow the page theme.").small().weak());
            return;
        }
        if shape.shape.is_cloud() {
            ui.label(
                RichText::new("Icons scale uniformly and keep their original appearance.")
                    .small()
                    .weak(),
            );
            self.text_style_section(
                ui,
                &shape.style,
                &bp_scene::shape_geometry(self.libraries, shape).style,
            );
        } else {
            let has_corners = self.libraries.resolve(&shape.shape).outline == Outline::Rect;
            self.style_section(ui, el, true, has_corners);
        }
    }

    fn position_size_section(&mut self, ui: &mut Ui, el: &Element) {
        let Some(shape) = el.as_shape() else { return };
        section(ui, "Position and size");
        let b = shape.bounds;
        let (mut x, mut y, mut w, mut h) = (b.x0, b.y0, b.width(), b.height());
        let mut moved = false;
        let cloud = shape.network_container.is_none() && shape.shape.is_cloud();
        let minimum = shape
            .network_container
            .map(bp_model::NetworkContainer::min_size)
            .unwrap_or(kurbo::Size::new(1.0, 1.0));
        let aspect = b.width() / b.height().max(1.0);
        egui::Grid::new("geometry").num_columns(4).show(ui, |ui| {
            ui.label("X");
            moved |= ui.add(DragValue::new(&mut x).speed(1.0)).changed();
            ui.label("Y");
            moved |= ui.add(DragValue::new(&mut y).speed(1.0)).changed();
            ui.end_row();
            ui.label("W");
            let width_changed = ui
                .add(DragValue::new(&mut w).range(minimum.width..=100_000.0))
                .changed();
            moved |= width_changed;
            if cloud && width_changed {
                h = w / aspect;
            }
            ui.label("H");
            let height_changed = ui
                .add(DragValue::new(&mut h).range(minimum.height..=100_000.0))
                .changed();
            moved |= height_changed;
            if cloud && height_changed {
                w = h * aspect;
            }
            ui.end_row();
        });
        if moved {
            let id = el.id;
            let bounds = Rect::new(x, y, x + w, y + h);
            let planned = if shape.network_container.is_some() {
                if x != b.x0 || y != b.y0 {
                    let target = match el.parent {
                        bp_model::Parent::Element(parent)
                            if self.doc.elements[&parent].network_container().is_some() =>
                        {
                            Some(parent)
                        }
                        _ => None,
                    };
                    bp_commands::network::drop(
                        &self.doc,
                        &[id],
                        target,
                        bounds.origin() - b.origin(),
                    )
                } else {
                    bp_commands::network::resize(&self.doc, id, bounds)
                }
            } else {
                bp_commands::network::validate_bounds(&self.doc, id, bounds).map(|()| {
                    vec![Command::Set {
                        id,
                        prop: Prop::Bounds(bounds),
                    }]
                })
            };
            match planned {
                Ok(commands) => self.apply_merging("Resize", format!("bounds:{id}"), commands),
                Err(error) => self.status = error.to_string(),
            }
        }
        ui.add_space(6.0);
    }

    fn connector_inspector(&mut self, ui: &mut Ui, el: &Element) {
        let Some(c) = el.as_connector() else { return };
        let id = el.id;
        ui.heading("Connector");
        ui.add_space(6.0);
        if self.workspace.inspector_tab == InspectorTab::Arrange {
            self.arrange_section(ui);
            return;
        }
        if self.workspace.inspector_tab == InspectorTab::Appearance {
            self.style_section(ui, el, false, false);
            return;
        }
        let erd = self.page_kind() == Some(DiagramKind::Erd);
        if erd {
            if self.workspace.relationship_in_inspector {
                self.relationship_details(ui);
            } else {
                ui.label(RichText::new("Relationship").strong());
                let mappings = self.foreign_key_mapping(id);
                if mappings.is_empty() {
                    ui.label(RichText::new("Diagram relationship").small().weak());
                }
                if !mappings.is_empty() {
                    ui.label(format!(
                        "SQL foreign key · {} mapped column{}",
                        mappings.len(),
                        if mappings.len() == 1 { "" } else { "s" }
                    ));
                }
                if ui.button("Relationship details").clicked() {
                    self.preferences.drawer_open = true;
                }
            }
            ui.separator();
        }

        section(ui, "Routing");
        ui.horizontal(|ui| {
            for routing in Routing::ALL {
                if ui
                    .selectable_label(c.routing == routing, routing.label())
                    .clicked()
                    && c.routing != routing
                {
                    // Waypoints from one routing mode rarely suit another.
                    self.apply(
                        "Change routing",
                        [
                            Command::Set {
                                id,
                                prop: Prop::Routing(routing),
                            },
                            Command::Set {
                                id,
                                prop: Prop::Waypoints(Vec::new()),
                            },
                        ],
                    );
                }
            }
        });
        if !c.waypoints.is_empty() && ui.button(labelled(icon::PATH, "Reset route")).clicked() {
            self.apply(
                "Reset route",
                [Command::Set {
                    id,
                    prop: Prop::Waypoints(Vec::new()),
                }],
            );
        }
        ui.add_space(6.0);

        if !erd {
            section(ui, "Ends");
            egui::Grid::new("ends").num_columns(2).show(ui, |ui| {
                for (label, current, is_start) in [
                    ("Start", c.start_marker, true),
                    ("End", c.end_marker, false),
                ] {
                    ui.label(label);
                    egui::ComboBox::from_id_salt(("marker", is_start))
                        .selected_text(current.label())
                        .show_ui(ui, |ui| {
                            for marker in Marker::ALL {
                                let is_erd = matches!(
                                    marker,
                                    Marker::ExactlyOne
                                        | Marker::ZeroOrOne
                                        | Marker::OneOrMany
                                        | Marker::ZeroOrMany
                                        | Marker::Many
                                );
                                if marker != Marker::None && is_erd != erd {
                                    continue;
                                }
                                if ui
                                    .selectable_label(current == marker, marker.label())
                                    .clicked()
                                    && current != marker
                                {
                                    let prop = if is_start {
                                        Prop::StartMarker(marker)
                                    } else {
                                        Prop::EndMarker(marker)
                                    };
                                    self.apply("Change marker", [Command::Set { id, prop }]);
                                }
                            }
                        });
                    ui.end_row();
                }
            });
        }
        if ui
            .button(labelled(icon::ARROWS_LEFT_RIGHT, "Swap ends"))
            .clicked()
        {
            let commands = vec![
                Command::Set {
                    id,
                    prop: Prop::ForeignKey(None),
                },
                Command::Set {
                    id,
                    prop: Prop::Source(c.target.clone()),
                },
                Command::Set {
                    id,
                    prop: Prop::Target(c.source.clone()),
                },
                Command::Set {
                    id,
                    prop: Prop::ForeignKey(c.foreign_key.clone().map(|mut key| {
                        key.owner_at_target = !key.owner_at_target;
                        key
                    })),
                },
                Command::Set {
                    id,
                    prop: Prop::StartMarker(c.end_marker),
                },
                Command::Set {
                    id,
                    prop: Prop::EndMarker(c.start_marker),
                },
                Command::Set {
                    id,
                    prop: Prop::Waypoints(c.waypoints.iter().rev().copied().collect()),
                },
            ];
            self.apply("Swap ends", commands);
        }
        ui.add_space(6.0);

        section(ui, "Label");
        if self.editing.is_none() {
            let mut text = c.text.clone();
            if ui
                .add(
                    crate::widgets::text_field(&mut text)
                        .hint_text("Label")
                        .desired_width(f32::INFINITY),
                )
                .changed()
            {
                self.apply_merging(
                    "Edit label",
                    format!("text:{id}"),
                    vec![Command::Set {
                        id,
                        prop: Prop::Text(text),
                    }],
                );
            }
        }
        let mut t = c.label_position;
        ui.horizontal(|ui| {
            ui.label("Position");
            if ui
                .add(egui::Slider::new(&mut t, 0.0..=1.0).show_value(false))
                .changed()
            {
                self.apply_merging(
                    "Move label",
                    format!("label:{id}"),
                    vec![Command::Set {
                        id,
                        prop: Prop::LabelPosition(t),
                    }],
                );
            }
        });
        ui.add_space(6.0);

        if !erd && self.page_kind() != Some(DiagramKind::Cloud) {
            self.style_section(ui, el, false, false);
        }
    }

    fn group_inspector(&mut self, ui: &mut Ui, el: &Element) {
        ui.heading("Group");
        let count = self.doc.descendants(el.id).len();
        ui.label(RichText::new(format!("{count} elements inside")).weak());
        ui.add_space(6.0);
        if self.workspace.inspector_tab == InspectorTab::Arrange {
            self.arrange_section(ui);
            return;
        }
        if ui.button(labelled(icon::SQUARES_FOUR, "Ungroup")).clicked() {
            self.ungroup_selection();
        }
        if ui
            .button(labelled(icon::SELECTION_PLUS, "Edit inside"))
            .on_hover_text("Or double-click a shape in the group")
            .clicked()
        {
            self.scope = Some(el.id);
            self.selection.clear();
        }
        ui.add_space(6.0);
    }

    fn multi_inspector(&mut self, ui: &mut Ui, many: &[Element]) {
        ui.heading(format!("{} selected", many.len()));
        ui.add_space(6.0);
        if self.workspace.inspector_tab == InspectorTab::Design {
            if let Some(first) = many.iter().find(|e| e.style().is_some()) {
                let first = first.clone();
                let artwork = many
                    .iter()
                    .any(|el| el.as_shape().is_some_and(|shape| shape.shape.is_cloud()));
                if artwork {
                    let style = first.style().unwrap().clone();
                    let resolved = self.resolved_style(&first);
                    self.text_style_section(ui, &style, &resolved);
                } else {
                    self.style_section(ui, &first, true, false);
                }
            }
            return;
        }
        section(ui, "Align");
        ui.horizontal_wrapped(|ui| {
            for (glyph, tip, how) in [
                (icon::ALIGN_LEFT, "Align left", Align::Left),
                (
                    icon::ALIGN_CENTER_HORIZONTAL,
                    "Align centres",
                    Align::CenterX,
                ),
                (icon::ALIGN_RIGHT, "Align right", Align::Right),
                (icon::ALIGN_TOP, "Align top", Align::Top),
                (icon::ALIGN_CENTER_VERTICAL, "Align middles", Align::CenterY),
                (icon::ALIGN_BOTTOM, "Align bottom", Align::Bottom),
            ] {
                if crate::widgets::icon_button(ui, glyph, tip).clicked() {
                    self.align(how);
                }
            }
        });
        ui.horizontal(|ui| {
            let enough = many.len() >= 3;
            if ui
                .add_enabled(
                    enough,
                    Button::new(labelled(icon::DOTS_THREE, "Distribute horizontally")),
                )
                .clicked()
            {
                self.distribute(true);
            }
        });
        let enough = many.len() >= 3;
        if ui
            .add_enabled(
                enough,
                Button::new(labelled(icon::DOTS_THREE_VERTICAL, "Distribute vertically")),
            )
            .clicked()
        {
            self.distribute(false);
        }
        ui.add_space(6.0);
        if ui.button(labelled(icon::BOUNDING_BOX, "Group")).clicked() {
            self.group_selection();
        }
        ui.add_space(6.0);
        self.arrange_section(ui);
    }

    /// Fill, stroke and text controls. Values show the first element's
    /// resolved style; changes apply to the whole selection.
    fn style_section(&mut self, ui: &mut Ui, el: &Element, with_fill: bool, with_corners: bool) {
        let Some(style) = el.style().cloned() else {
            return;
        };
        let resolved = self.resolved_style(el);
        section(ui, "Style");
        egui::Grid::new("style").num_columns(3).show(ui, |ui| {
            if with_fill {
                ui.label("Fill");
                let mut fill = resolved.fill;
                if optional_color(ui, &mut fill) {
                    self.set_on_selection("Fill", true, |e| {
                        e.style().map(|_| Prop::Fill(Some(Paint::from(fill))))
                    });
                }
                self.reset_button(ui, style.fill.is_some(), "Fill", |_| Prop::Fill(None));
                ui.end_row();
            }
            ui.label("Line");
            let mut stroke = resolved.stroke;
            if optional_color(ui, &mut stroke) {
                self.set_on_selection("Line colour", true, |e| {
                    e.style().map(|_| Prop::Stroke(Some(Paint::from(stroke))))
                });
            }
            self.reset_button(ui, style.stroke.is_some(), "Line colour", |_| {
                Prop::Stroke(None)
            });
            ui.end_row();

            ui.label("Width");
            let mut width = resolved.stroke_width;
            if ui
                .add(
                    DragValue::new(&mut width)
                        .range(0.0..=40.0)
                        .speed(0.1)
                        .max_decimals(1),
                )
                .changed()
            {
                self.set_on_selection("Line width", true, |e| {
                    e.style().map(|_| Prop::StrokeWidth(Some(width)))
                });
            }
            self.reset_button(ui, style.stroke_width.is_some(), "Line width", |_| {
                Prop::StrokeWidth(None)
            });
            ui.end_row();

            ui.label("Dash");
            egui::ComboBox::from_id_salt("dash")
                .selected_text(resolved.dash.label())
                .show_ui(ui, |ui| {
                    for dash in Dash::ALL {
                        if ui
                            .selectable_label(resolved.dash == dash, dash.label())
                            .clicked()
                        {
                            self.set_on_selection("Dash", false, |e| {
                                e.style().map(|_| Prop::Dash(Some(dash)))
                            });
                        }
                    }
                });
            self.reset_button(ui, style.dash.is_some(), "Dash", |_| Prop::Dash(None));
            ui.end_row();

            if with_corners {
                ui.label("Corners");
                let mut radius = resolved.corner_radius;
                if ui
                    .add(DragValue::new(&mut radius).range(0.0..=500.0).speed(0.5))
                    .changed()
                {
                    self.set_on_selection("Corner radius", true, |e| {
                        e.style().map(|_| Prop::CornerRadius(Some(radius)))
                    });
                }
                self.reset_button(ui, style.corner_radius.is_some(), "Corner radius", |_| {
                    Prop::CornerRadius(None)
                });
                ui.end_row();
            }

            ui.label("Opacity");
            let mut opacity = resolved.opacity * 100.0;
            if ui
                .add(
                    egui::Slider::new(&mut opacity, 0.0..=100.0)
                        .suffix("%")
                        .max_decimals(0),
                )
                .changed()
            {
                self.set_on_selection("Opacity", true, |e| {
                    e.style().map(|_| Prop::Opacity(Some(opacity / 100.0)))
                });
            }
            self.reset_button(ui, style.opacity.is_some(), "Opacity", |_| {
                Prop::Opacity(None)
            });
            ui.end_row();

            if with_fill {
                ui.label("Shadow");
                let mut shadow = resolved.shadow;
                if ui.checkbox(&mut shadow, "").changed() {
                    self.set_on_selection("Shadow", false, |e| {
                        e.style().map(|_| Prop::Shadow(Some(shadow)))
                    });
                }
                self.reset_button(ui, style.shadow.is_some(), "Shadow", |_| Prop::Shadow(None));
                ui.end_row();
            }
        });
        ui.add_space(6.0);
        self.text_style_section(ui, &style, &resolved);
    }

    fn text_style_section(&mut self, ui: &mut Ui, style: &Style, resolved: &StyleValues) {
        section(ui, "Text style");
        egui::Grid::new("text-style").num_columns(3).show(ui, |ui| {
            ui.label("Size");
            let mut size = resolved.font_size;
            if ui
                .add(
                    DragValue::new(&mut size)
                        .range(4.0..=400.0)
                        .speed(0.2)
                        .max_decimals(1),
                )
                .changed()
            {
                self.set_on_selection("Font size", true, |e| {
                    e.style().map(|_| Prop::FontSize(Some(size)))
                });
            }
            self.reset_button(ui, style.font_size.is_some(), "Font size", |_| {
                Prop::FontSize(None)
            });
            ui.end_row();

            ui.label("Colour");
            let mut color = resolved.text_color;
            if color_button(ui, &mut color) {
                self.set_on_selection("Text colour", true, |e| {
                    e.style().map(|_| Prop::TextColor(Some(color)))
                });
            }
            self.reset_button(ui, style.text_color.is_some(), "Text colour", |_| {
                Prop::TextColor(None)
            });
            ui.end_row();

            ui.label("Style");
            ui.horizontal(|ui| {
                if style_toggle(ui, resolved.bold, icon::TEXT_B, "Bold").clicked() {
                    let bold = !resolved.bold;
                    self.set_on_selection("Bold", false, |e| {
                        e.style().map(|_| Prop::Bold(Some(bold)))
                    });
                }
                if style_toggle(ui, resolved.italic, icon::TEXT_ITALIC, "Italic").clicked() {
                    let italic = !resolved.italic;
                    self.set_on_selection("Italic", false, |e| {
                        e.style().map(|_| Prop::Italic(Some(italic)))
                    });
                }
            });
            ui.label("");
            ui.end_row();

            ui.label("Align");
            ui.horizontal(|ui| {
                for (glyph, align, label) in [
                    (icon::TEXT_ALIGN_LEFT, TextAlign::Left, "Align text left"),
                    (
                        icon::TEXT_ALIGN_CENTER,
                        TextAlign::Center,
                        "Align text center",
                    ),
                    (icon::TEXT_ALIGN_RIGHT, TextAlign::Right, "Align text right"),
                ] {
                    if style_toggle(ui, resolved.text_align == align, glyph, label).clicked() {
                        self.set_on_selection("Text alignment", false, |e| {
                            e.style().map(|_| Prop::TextAlign(Some(align)))
                        });
                    }
                }
            });
            self.reset_button(ui, style.text_align.is_some(), "Text alignment", |_| {
                Prop::TextAlign(None)
            });
            ui.end_row();

            ui.label("Vertical");
            ui.horizontal(|ui| {
                for (glyph, align, label) in [
                    (icon::ALIGN_TOP, VerticalAlign::Top, "Align text top"),
                    (
                        icon::ALIGN_CENTER_VERTICAL,
                        VerticalAlign::Middle,
                        "Align text middle",
                    ),
                    (
                        icon::ALIGN_BOTTOM,
                        VerticalAlign::Bottom,
                        "Align text bottom",
                    ),
                ] {
                    if style_toggle(ui, resolved.vertical_align == align, glyph, label).clicked() {
                        self.set_on_selection("Vertical alignment", false, |e| {
                            e.style().map(|_| Prop::VerticalAlign(Some(align)))
                        });
                    }
                }
            });
            self.reset_button(
                ui,
                style.vertical_align.is_some(),
                "Vertical alignment",
                |_| Prop::VerticalAlign(None),
            );
            ui.end_row();
        });
        ui.add_space(6.0);
    }

    fn arrange_section(&mut self, ui: &mut Ui) {
        section(ui, "Arrange");
        ui.horizontal_wrapped(|ui| {
            for (glyph, tip, how) in [
                (icon::ARROW_LINE_UP, "Bring to front", Reorder::Front),
                (icon::ARROW_UP, "Bring forward", Reorder::Forward),
                (icon::ARROW_DOWN, "Send backward", Reorder::Backward),
                (icon::ARROW_LINE_DOWN, "Send to back", Reorder::Back),
            ] {
                if crate::widgets::icon_button(ui, glyph, tip).clicked() {
                    self.reorder(how);
                }
            }
            let locked = self
                .selection
                .iter()
                .all(|id| self.doc.elements.get(id).is_some_and(|e| e.locked));
            let (glyph, tip) = if locked {
                (icon::LOCK, "Unlock")
            } else {
                (icon::LOCK_OPEN, "Lock")
            };
            if style_toggle(ui, locked, glyph, tip).clicked() {
                self.toggle_lock();
            }
            if crate::widgets::icon_button(ui, icon::TRASH, "Delete").clicked() {
                self.delete_selection();
            }
        });
    }

    /// The fully resolved style of `el`, as the scene draws it.
    fn resolved_style(&self, el: &Element) -> StyleValues {
        match &el.kind {
            ElementKind::Shape(s) => {
                let def = self.libraries.resolve(&s.shape);
                s.style.resolve(&def.default_style())
            }
            ElementKind::Connector(c) => c.style.resolve(&bp_scene::connector_defaults()),
            ElementKind::Group => StyleValues::default(),
        }
    }

    /// A small button that clears an override, shown only when there is one.
    fn reset_button(
        &mut self,
        ui: &mut Ui,
        overridden: bool,
        label: &str,
        prop: impl Fn(&Element) -> Prop,
    ) {
        if overridden {
            if crate::widgets::icon_button(
                ui,
                icon::ARROW_COUNTER_CLOCKWISE,
                &format!("Reset {label} to the shape's default"),
            )
            .clicked()
            {
                self.set_on_selection(label, false, |e| e.style().map(|_| prop(e)));
            }
        } else {
            ui.label("");
        }
    }
}

fn style_toggle(ui: &mut Ui, selected: bool, glyph: &str, label: &str) -> egui::Response {
    let response = ui.selectable_label(selected, crate::theme::icon(glyph));
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            ui.is_enabled(),
            selected,
            label,
        )
    });
    crate::widgets::focus_outline(ui, &response);
    response.on_hover_text(label)
}

fn section(ui: &mut Ui, title: &str) {
    ui.label(
        RichText::new(title)
            .small()
            .strong()
            .color(ui.visuals().weak_text_color()),
    );
}

pub fn color_button(ui: &mut Ui, color: &mut Color) -> bool {
    let mut rgba = [color.r, color.g, color.b, color.a];
    let changed = ui.color_edit_button_srgba_unmultiplied(&mut rgba).changed();
    if changed {
        *color = Color::rgba(rgba[0], rgba[1], rgba[2], rgba[3]);
    }
    changed
}

/// A colour picker with a "none" toggle.
fn optional_color(ui: &mut Ui, value: &mut Option<Color>) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        let mut on = value.is_some();
        let mut color = value.unwrap_or(Color::WHITE);
        changed |= ui.checkbox(&mut on, "").changed();
        ui.add_enabled_ui(on, |ui| changed |= color_button(ui, &mut color));
        *value = on.then_some(color);
    });
    changed
}
