//! ERD relationship tools and their vector symbols.

use crate::app::{BlueprintApp, Tool};
use crate::theme::ACCENT;
use bp_model::{Connector, DiagramKind, Marker};
use egui::{Button, Color32, Pos2, Rect, Response, RichText, Sense, Stroke, Ui, Vec2};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ErdConnection {
    #[default]
    ExactlyOne,
    ZeroOrOne,
    OneOrMany,
    ZeroOrMany,
    Many,
}

impl ErdConnection {
    pub const ALL: [Self; 5] = [
        Self::ExactlyOne,
        Self::ZeroOrOne,
        Self::OneOrMany,
        Self::ZeroOrMany,
        Self::Many,
    ];

    pub fn marker(self) -> Marker {
        match self {
            Self::ExactlyOne => Marker::ExactlyOne,
            Self::ZeroOrOne => Marker::ZeroOrOne,
            Self::OneOrMany => Marker::OneOrMany,
            Self::ZeroOrMany => Marker::ZeroOrMany,
            Self::Many => Marker::Many,
        }
    }

    pub fn from_marker(marker: Marker) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.marker() == marker)
    }

    pub fn label(self) -> &'static str {
        self.marker().label()
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::ExactlyOne => "Exactly one (1): one related record is required.",
            Self::ZeroOrOne => {
                "Zero or one (0..1): a related record is optional, with at most one."
            }
            Self::OneOrMany => "One or many (1..*): at least one related record is required.",
            Self::ZeroOrMany => {
                "Zero or many (0..*): any number of related records, including none."
            }
            Self::Many => "Many (*): multiple related records; no minimum is specified.",
        }
    }

    pub fn next(self) -> Self {
        let index = Self::ALL.iter().position(|kind| *kind == self).unwrap();
        Self::ALL[(index + 1) % Self::ALL.len()]
    }
}

impl BlueprintApp {
    pub fn select_erd_connection(&mut self, kind: ErdConnection) {
        if self.page_kind() != Some(DiagramKind::Erd) {
            return;
        }
        self.finish_text_edit(true);
        self.cancel_drag();
        self.erd_connection = kind;
        self.tool = Tool::Connector;
    }

    pub fn cycle_erd_connection(&mut self) {
        self.select_erd_connection(self.erd_connection.next());
    }

    /// Direct row-port drags in Select retain inferred FK cardinalities.
    /// The explicit relationship tool always honors the user's chosen endpoint.
    pub(crate) fn configure_connection(&self, connector: &mut Connector) {
        if self.page_kind() != Some(DiagramKind::Erd) {
            return;
        }
        let relationship = self.table_relationship(&connector.source, &connector.target);
        if self.tool == Tool::Connector || relationship.is_none() {
            connector.start_marker = Marker::ExactlyOne;
            connector.end_marker = self.erd_connection.marker();
        } else if let Some((start, end, ..)) = relationship {
            connector.start_marker = start;
            connector.end_marker = end;
        }
    }

    pub fn erd_connection_palette(&mut self, ui: &mut Ui) {
        ui.label(RichText::new("Connections").small().strong());
        ui.label(
            RichText::new("C to connect · Shift+C to cycle")
                .small()
                .weak(),
        );
        ui.add_space(4.0);
        let columns = ((ui.available_width() / 96.0).floor() as usize).clamp(1, 5);
        let width = (ui.available_width() - 4.0 * (columns - 1) as f32) / columns as f32;
        egui::Grid::new("erd-connection-palette")
            .spacing(Vec2::splat(4.0))
            .show(ui, |ui| {
                for (index, kind) in ErdConnection::ALL.into_iter().enumerate() {
                    if index > 0 && index % columns == 0 {
                        ui.end_row();
                    }
                    if connection_button(ui, kind, self.erd_connection == kind, width, true)
                        .clicked()
                    {
                        self.select_erd_connection(kind);
                    }
                }
            });
    }

    pub fn erd_connection_toolbar(&mut self, ui: &mut Ui) {
        let kind = self.erd_connection;
        let active = self.tool == Tool::Connector;
        let (response, _) = egui::containers::menu::MenuButton::from_button(
            Button::new("")
                .selected(active)
                .min_size(Vec2::new(160.0, 28.0)),
        )
        .ui(ui, |ui| {
            for option in ErdConnection::ALL {
                if connection_button(ui, option, option == kind, 176.0, false).clicked() {
                    self.select_erd_connection(option);
                    ui.close();
                }
            }
        });
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                true,
                format!("ERD connection: {}", kind.label()),
            )
        });
        let rect = response.rect;
        let color = if active {
            ACCENT
        } else {
            ui.visuals().text_color()
        };
        paint_connection_icon(
            ui.painter(),
            Rect::from_center_size(
                Pos2::new(rect.left() + 27.0, rect.center().y),
                Vec2::new(40.0, 18.0),
            ),
            kind,
            color,
            ui.style()
                .interact_selectable(&response, active)
                .weak_bg_fill,
        );
        ui.painter().text(
            Pos2::new(rect.left() + 51.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            kind.label(),
            egui::FontId::proportional(12.0),
            color,
        );
        let center = Pos2::new(rect.right() - 9.0, rect.center().y);
        ui.painter().line_segment(
            [center + Vec2::new(-3.0, -1.0), center + Vec2::new(0.0, 2.0)],
            Stroke::new(1.2, color),
        );
        ui.painter().line_segment(
            [center + Vec2::new(0.0, 2.0), center + Vec2::new(3.0, -1.0)],
            Stroke::new(1.2, color),
        );
        response.on_hover_text(format!(
            "{}\nThe symbol applies at the end you drag to. C: connect. Shift+C: next type.",
            kind.description()
        ));
    }
}

fn connection_button(
    ui: &mut Ui,
    kind: ErdConnection,
    selected: bool,
    width: f32,
    cell: bool,
) -> Response {
    let height = if cell { 56.0 } else { 32.0 };
    let response = ui.add_sized(
        [width, height],
        Button::new("").selected(selected).sense(Sense::click()),
    );
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, kind.label())
    });
    let color = if selected {
        ACCENT
    } else {
        ui.visuals().text_color()
    };
    let rect = response.rect;
    let center = if cell {
        Pos2::new(rect.center().x, rect.top() + 19.0)
    } else {
        Pos2::new(rect.left() + 33.0, rect.center().y)
    };
    paint_connection_icon(
        ui.painter(),
        Rect::from_center_size(center, Vec2::new(52.0, 20.0)),
        kind,
        color,
        ui.style()
            .interact_selectable(&response, selected)
            .weak_bg_fill,
    );
    let (at, align) = if cell {
        (
            Pos2::new(rect.center().x, rect.top() + 43.0),
            egui::Align2::CENTER_CENTER,
        )
    } else {
        (
            Pos2::new(rect.left() + 68.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
        )
    };
    ui.painter().text(
        at,
        align,
        kind.label(),
        egui::FontId::proportional(11.0),
        color,
    );
    response.on_hover_text(format!("{}\nClick, then drag from one shape or column to another. The chosen symbol appears at the end you drag to.", kind.description()))
}

/// A relationship from one on the left to the chosen cardinality on the right.
fn paint_connection_icon(
    painter: &egui::Painter,
    rect: Rect,
    kind: ErdConnection,
    color: Color32,
    background: Color32,
) {
    let stroke = Stroke::new(1.4, color);
    let y = rect.center().y;
    let left = rect.left() + 1.0;
    let right = rect.right() - 1.0;
    let bar = |x| painter.line_segment([Pos2::new(x, y - 5.0), Pos2::new(x, y + 5.0)], stroke);
    painter.line_segment([Pos2::new(left, y), Pos2::new(right, y)], stroke);
    bar(left + 2.0);
    bar(left + 7.0);
    if matches!(
        kind,
        ErdConnection::OneOrMany | ErdConnection::ZeroOrMany | ErdConnection::Many
    ) {
        let hub = Pos2::new(right - 9.0, y);
        painter.line_segment([Pos2::new(right, y - 5.0), hub], stroke);
        painter.line_segment([hub, Pos2::new(right, y + 5.0)], stroke);
    } else {
        bar(right - 2.0);
    }
    match kind {
        ErdConnection::ExactlyOne => {
            bar(right - 8.0);
        }
        ErdConnection::OneOrMany => {
            bar(right - 13.0);
        }
        ErdConnection::ZeroOrOne | ErdConnection::ZeroOrMany => {
            let x = right
                - if kind == ErdConnection::ZeroOrOne {
                    10.0
                } else {
                    16.0
                };
            painter.circle(Pos2::new(x, y), 3.2, background, stroke);
        }
        ErdConnection::Many => {}
    }
}
