//! Compact controls with human-readable accessibility names.
use egui::{Response, Stroke, StrokeKind, Ui};

pub fn text_field(value: &mut dyn egui::TextBuffer) -> egui::TextEdit<'_> {
    egui::TextEdit::singleline(value).min_size(egui::vec2(0.0, 28.0))
}

pub fn icon_button(ui: &mut Ui, glyph: &str, label: &str) -> Response {
    let response = ui.add(
        egui::Button::new(crate::theme::icon(glyph).size(18.0)).min_size(egui::vec2(28.0, 28.0)),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    focus_outline(ui, &response);
    command_focus(ui, &response);
    response.on_hover_text(label)
}
pub fn focus_outline(ui: &Ui, response: &Response) {
    if response.has_focus() {
        ui.painter().rect_stroke(
            response.rect.shrink(1.0),
            4.0,
            Stroke::new(2.0, crate::theme::ACCENT),
            StrokeKind::Inside,
        );
    }
}
pub fn panel_title(ui: &mut Ui, title: &str) {
    ui.label(egui::RichText::new(title).size(14.0).strong());
}

pub fn primary_icon_button(ui: &mut Ui, glyph: &str, label: &str) -> Response {
    let response = ui.add(
        egui::Button::new(
            crate::theme::icon(glyph)
                .size(18.0)
                .color(egui::Color32::WHITE),
        )
        .fill(crate::theme::PRIMARY)
        .min_size(egui::vec2(28.0, 28.0)),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    focus_outline(ui, &response);
    command_focus(ui, &response);
    response.on_hover_text(label)
}

/// Buttons own activation keys, but must not suppress document shortcuts as
/// text fields do. Store only a focused control ID; a different field cannot
/// inherit permission merely because the previous control had focus.
pub fn command_focus(ui: &Ui, response: &Response) {
    if response.has_focus() {
        ui.ctx().data_mut(|data| {
            data.insert_temp(egui::Id::new("blueprint.control-focus"), response.id)
        });
    }
}

pub fn document_control_has_focus(ctx: &egui::Context) -> bool {
    let focused = ctx.memory(|memory| memory.focused());
    focused.is_some()
        && focused
            == ctx.data(|data| data.get_temp::<egui::Id>(egui::Id::new("blueprint.control-focus")))
}
