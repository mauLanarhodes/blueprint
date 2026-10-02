//! Blueprint's look: Inter for the UI, Phosphor icons, and a calmer light
//! theme than egui's developer-tool default.

use egui::{Color32, CornerRadius, FontFamily, RichText, Stroke, Theme, Vec2};

/// The font family egui-phosphor registers.
const ICON_FAMILY: &str = "phosphor";

/// Selection, handles and active controls.
pub const ACCENT: Color32 = Color32::from_rgb(37, 99, 235);
/// Smart guides while dragging.
pub const GUIDE: Color32 = Color32::from_rgb(236, 72, 153);
/// Ports and connection targets.
pub const PORT: Color32 = Color32::from_rgb(14, 165, 233);
pub const GRID: Color32 = Color32::from_gray(234);
pub const CANVAS_EDGE: Color32 = Color32::from_rgb(226, 232, 240);

pub fn install(ctx: &egui::Context) {
    let mut fonts = bp_render_egui::font_definitions();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    // Icons are drawn from their own family (see [`icon`]), never mixed
    // into text: Inter has glyphs in the Private Use Area where the icons
    // live, and the icon font has letter glyphs for its ligatures, so in
    // one fallback chain either font would break the other.
    if let Some(ui) = fonts.families.get_mut(&FontFamily::Proportional) {
        ui.retain(|f| f != ICON_FAMILY);
    }
    // egui-phosphor's own family puts the text font first; ours holds only
    // icons (labels next to icons are separate pieces of text).
    fonts.families.insert(
        FontFamily::Name(ICON_FAMILY.into()),
        vec![ICON_FAMILY.to_owned()],
    );
    ctx.set_fonts(fonts);
    ctx.set_theme(Theme::Light);
    ctx.style_mut_of(Theme::Light, |style| {
        let v = &mut style.visuals;
        v.panel_fill = Color32::from_rgb(248, 250, 252);
        v.window_fill = Color32::WHITE;
        v.faint_bg_color = Color32::from_rgb(241, 245, 249);
        v.extreme_bg_color = Color32::WHITE;
        v.window_corner_radius = CornerRadius::same(8);
        v.selection.bg_fill = Color32::from_rgb(219, 234, 254);
        v.selection.stroke = Stroke::new(1.0, ACCENT);
        v.hyperlink_color = ACCENT;
        for w in [
            &mut v.widgets.noninteractive,
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
        ] {
            w.corner_radius = CornerRadius::same(5);
        }
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, CANVAS_EDGE);
        v.widgets.inactive.weak_bg_fill = Color32::from_rgb(241, 245, 249);
        v.widgets.inactive.bg_fill = Color32::from_rgb(241, 245, 249);
        v.widgets.hovered.weak_bg_fill = Color32::from_rgb(226, 232, 240);
        v.widgets.hovered.bg_fill = Color32::from_rgb(226, 232, 240);
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(203, 213, 225));
        v.widgets.active.weak_bg_fill = Color32::from_rgb(219, 234, 254);
        v.widgets.active.bg_fill = Color32::from_rgb(219, 234, 254);
        let s = &mut style.spacing;
        s.item_spacing = Vec2::new(8.0, 6.0);
        s.button_padding = Vec2::new(8.0, 4.0);
        s.interact_size.y = 24.0;
    });
}

/// Whether the fonts from [`install`] are active. They arrive on the frame
/// after installing, and drawing with a family egui doesn't know panics.
pub fn fonts_ready(ctx: &egui::Context) -> bool {
    let icons = FontFamily::Name(ICON_FAMILY.into());
    bp_render_egui::bundled_fonts_ready(ctx) && ctx.fonts(|f| f.families().contains(&icons))
}

/// A Phosphor icon (`egui_phosphor::regular::*`) as text.
pub fn icon(glyph: &str) -> RichText {
    RichText::new(glyph).family(FontFamily::Name(ICON_FAMILY.into()))
}

/// An icon followed by a label, as button contents.
pub fn labelled(glyph: &str, label: &str) -> (RichText, String) {
    (icon(glyph), label.to_owned())
}
