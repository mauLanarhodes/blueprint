//! Docked workspace composition and responsive panel allocation.
use crate::preferences::{LeftTab, WorkspaceTheme};
use crate::{BlueprintApp, InspectorTab};
use egui::{Frame, Margin, Panel, Rect, Ui};
use egui_phosphor::regular as icon;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Dock {
    Left,
    Right,
}

pub struct WorkspaceState {
    pub inspector_tab: InspectorTab,
    pub selected_column: Option<(bp_model::ElementId, bp_model::ColumnId)>,
    pub relationship_in_inspector: bool,
    pub left_rect: Rect,
    pub right_rect: Rect,
    pub tool_rect: Rect,
    pub drawer_rect: Rect,
    pub effective_left_open: bool,
    pub effective_right_open: bool,
    pub(crate) preferred_dock: Option<Dock>,
    pub(crate) navigator: crate::navigator::NavigatorState,
    pub(crate) applied_theme: Option<WorkspaceTheme>,
}
impl Default for WorkspaceState {
    fn default() -> Self {
        Self {
            inspector_tab: InspectorTab::default(),
            selected_column: None,
            relationship_in_inspector: false,
            left_rect: Rect::NOTHING,
            right_rect: Rect::NOTHING,
            tool_rect: Rect::NOTHING,
            drawer_rect: Rect::NOTHING,
            effective_left_open: true,
            effective_right_open: true,
            preferred_dock: None,
            navigator: Default::default(),
            applied_theme: None,
        }
    }
}

fn panel_frame(ui: &Ui) -> Frame {
    Frame::side_top_panel(ui.style()).inner_margin(Margin::same(8))
}

/// egui remembers rendered panel sizes. Restore the user's preferred size
/// before layout so a temporary compact-window clamp cannot become permanent.
fn preferred_size(ctx: &egui::Context, name: &str, size: f32, vertical: bool) {
    if ctx.input(|input| input.pointer.any_down() || input.pointer.any_released()) {
        return;
    }
    let id = egui::Id::new(name);
    ctx.data_mut(|data| {
        if let Some(mut state) = data.get_persisted::<egui::containers::panel::PanelState>(id) {
            if vertical {
                state.outer_rect.max.y = state.outer_rect.min.y + size;
            } else {
                state.outer_rect.max.x = state.outer_rect.min.x + size;
            }
            data.insert_persisted(id, state);
        }
    });
}

impl BlueprintApp {
    pub(crate) fn toggle_dock(&mut self, dock: Dock) {
        match dock {
            Dock::Left => self.preferences.left_open = !self.workspace.effective_left_open,
            Dock::Right => self.preferences.right_open = !self.workspace.effective_right_open,
        }
        self.workspace.preferred_dock = Some(dock);
    }

    pub(crate) fn workspace_ui(&mut self, ui: &mut Ui) {
        if self.workspace.applied_theme != Some(self.preferences.theme) {
            crate::theme::apply(ui.ctx(), self.preferences.theme);
            self.workspace.applied_theme = Some(self.preferences.theme);
        }
        Panel::top("menu_bar")
            .exact_size(28.0)
            .resizable(false)
            .frame(Frame::side_top_panel(ui.style()).inner_margin(Margin::symmetric(8, 0)))
            .show(ui, |ui| {
                ui.spacing_mut().interact_size.y = 24.0;
                self.menu_bar(ui);
            });
        Panel::top("document_bar")
            .exact_size(40.0)
            .resizable(false)
            .frame(Frame::side_top_panel(ui.style()).inner_margin(Margin::symmetric(8, 6)))
            .show(ui, |ui| self.document_bar(ui));
        Panel::bottom("status_bar")
            .exact_size(28.0)
            .resizable(false)
            .frame(Frame::side_top_panel(ui.style()).inner_margin(Margin::symmetric(8, 0)))
            .show(ui, |ui| self.status_bar(ui));
        let available = ui.available_width();
        let mut left = self.preferences.left_open;
        let mut right = self.preferences.right_open;
        let fits = |left: bool, right: bool| {
            available - if left { 220.0 } else { 28.0 } - if right { 260.0 } else { 28.0 } >= 360.0
        };
        if !fits(left, right) {
            match self.workspace.preferred_dock {
                Some(Dock::Left) => right = false,
                _ => left = false,
            }
        }
        if !fits(left, right) {
            left = false;
            right = false;
        }
        self.workspace.effective_left_open = left;
        self.workspace.effective_right_open = right;
        let right_budget = if right {
            self.preferences
                .right_width
                .min((available - if left { 220.0 } else { 28.0 } - 360.0).max(260.0))
        } else {
            28.0
        };
        let max_left = (available - right_budget - 360.0).clamp(220.0, 400.0);
        if left {
            preferred_size(
                ui.ctx(),
                "workspace_left",
                self.preferences.left_width.min(max_left),
                false,
            );
            let shown = Panel::left("workspace_left")
                .default_size(self.preferences.left_width)
                .size_range(220.0..=max_left)
                .resizable(true)
                .frame(panel_frame(ui))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.selectable_value(
                            &mut self.preferences.left_tab,
                            LeftTab::Library,
                            "Library",
                        );
                        ui.selectable_value(
                            &mut self.preferences.left_tab,
                            LeftTab::Navigator,
                            "Navigator",
                        );
                        if crate::widgets::icon_button(ui, icon::CARET_LEFT, "Collapse library")
                            .clicked()
                        {
                            self.toggle_dock(Dock::Left);
                        }
                    });
                    ui.separator();
                    match self.preferences.left_tab {
                        LeftTab::Library => self.palette(ui),
                        LeftTab::Navigator => self.navigator(ui),
                    }
                });
            self.workspace.left_rect = shown.response.rect;
            if max_left >= self.preferences.left_width
                || ui
                    .ctx()
                    .is_being_dragged(egui::Id::new("workspace_left").with("__resize"))
            {
                self.preferences.left_width = shown.response.rect.width().clamp(220.0, 400.0);
            }
        } else {
            let shown = Panel::left("library_rail")
                .exact_size(28.0)
                .resizable(false)
                .frame(Frame::side_top_panel(ui.style()).inner_margin(0))
                .show(ui, |ui| {
                    if crate::widgets::icon_button(ui, icon::CARET_RIGHT, "Expand library")
                        .clicked()
                    {
                        self.toggle_dock(Dock::Left);
                    }
                });
            self.workspace.left_rect = shown.response.rect;
        }
        let relationship = self.page_kind() == Some(bp_model::DiagramKind::Erd)
            && self.selection.len() == 1
            && self
                .doc
                .elements
                .get(&self.selection[0])
                .is_some_and(|e| e.is_connector());
        let drawer_fits = ui.available_height() >= 416.0;
        self.workspace.relationship_in_inspector = relationship && !drawer_fits;
        let max_right = (ui.available_width() - 360.0).clamp(260.0, 440.0);
        if right {
            preferred_size(
                ui.ctx(),
                "workspace_right",
                self.preferences.right_width.min(max_right),
                false,
            );
            let shown = Panel::right("workspace_right")
                .default_size(self.preferences.right_width)
                .size_range(260.0..=max_right)
                .resizable(true)
                .frame(panel_frame(ui))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        crate::widgets::panel_title(ui, "Properties");
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if crate::widgets::icon_button(
                                ui,
                                icon::CARET_RIGHT,
                                "Collapse inspector",
                            )
                            .clicked()
                            {
                                self.toggle_dock(Dock::Right);
                            }
                        });
                    });
                    ui.separator();
                    self.inspector(ui);
                });
            self.workspace.right_rect = shown.response.rect;
            if max_right >= self.preferences.right_width
                || ui
                    .ctx()
                    .is_being_dragged(egui::Id::new("workspace_right").with("__resize"))
            {
                self.preferences.right_width = shown.response.rect.width().clamp(260.0, 440.0);
            }
        } else {
            let shown = Panel::right("inspector_rail")
                .exact_size(28.0)
                .resizable(false)
                .frame(Frame::side_top_panel(ui.style()).inner_margin(0))
                .show(ui, |ui| {
                    if crate::widgets::icon_button(ui, icon::CARET_LEFT, "Expand inspector")
                        .clicked()
                    {
                        self.toggle_dock(Dock::Right);
                    }
                });
            self.workspace.right_rect = shown.response.rect;
        }
        self.workspace.drawer_rect = Rect::NOTHING;
        if relationship && self.preferences.drawer_open && drawer_fits {
            let max_height = (ui.available_height() - 240.0).clamp(120.0, 360.0);
            preferred_size(
                ui.ctx(),
                "relationship_drawer",
                self.preferences.drawer_height.min(max_height),
                true,
            );
            let drawer = Panel::bottom("relationship_drawer")
                .default_size(self.preferences.drawer_height)
                .size_range(120.0..=max_height)
                .resizable(true)
                .frame(panel_frame(ui))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        crate::widgets::panel_title(ui, "Relationship details");
                        if crate::widgets::icon_button(
                            ui,
                            icon::CARET_DOWN,
                            "Collapse relationship details",
                        )
                        .clicked()
                        {
                            self.preferences.drawer_open = false;
                        }
                    });
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .id_salt("relationship_scroll")
                        .show(ui, |ui| self.relationship_details(ui));
                });
            self.workspace.drawer_rect = drawer.response.rect;
            if max_height >= self.preferences.drawer_height
                || ui
                    .ctx()
                    .is_being_dragged(egui::Id::new("relationship_drawer").with("__resize"))
            {
                self.preferences.drawer_height = drawer.response.rect.height().clamp(120.0, 360.0);
            }
        }
        egui::CentralPanel::no_frame().show(ui, |ui| self.canvas(ui));
    }
}
