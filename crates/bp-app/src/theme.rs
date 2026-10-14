//! Inter typography, Phosphor icons and shared workspace tokens.
use crate::preferences::WorkspaceTheme;
use egui::{Color32, CornerRadius, FontFamily, FontId, RichText, Stroke, Theme, Vec2};
const ICON_FAMILY: &str = "phosphor";
pub const ACCENT: Color32 = Color32::from_rgb(0x4c, 0x9a, 0xff);
pub const GUIDE: Color32 = Color32::from_rgb(236, 72, 153);
pub const PORT: Color32 = Color32::from_rgb(14, 165, 233);
pub const SUCCESS: Color32 = Color32::from_rgb(0x36, 0xcb, 0xb2);
pub const WARNING: Color32 = Color32::from_rgb(0xe9, 0xb9, 0x49);
pub const ERROR: Color32 = Color32::from_rgb(0xff, 0x71, 0x78);
pub const PRIMARY: Color32 = Color32::from_rgb(0x17, 0x6b, 0xd6);

pub fn install(ctx: &egui::Context) {
    let mut fonts = bp_render_egui::font_definitions();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    // Never mix icon glyphs with Inter's private-use glyphs.
    if let Some(ui) = fonts.families.get_mut(&FontFamily::Proportional) {
        ui.retain(|f| f != ICON_FAMILY);
    }
    fonts.families.insert(
        FontFamily::Name(ICON_FAMILY.into()),
        vec![ICON_FAMILY.to_owned()],
    );
    ctx.set_fonts(fonts);
    for theme in [Theme::Dark, Theme::Light] {
        ctx.style_mut_of(theme, |style| {
            let dark = theme == Theme::Dark;
            let panel = if dark {
                Color32::from_rgb(0x1c, 0x22, 0x2a)
            } else {
                Color32::from_rgb(248, 250, 252)
            };
            let elevated = if dark {
                Color32::from_rgb(0x25, 0x2d, 0x37)
            } else {
                Color32::from_rgb(241, 245, 249)
            };
            let border = if dark {
                Color32::from_rgb(0x39, 0x44, 0x51)
            } else {
                Color32::from_rgb(203, 213, 225)
            };
            let text = if dark {
                Color32::from_rgb(0xee, 0xf2, 0xf6)
            } else {
                Color32::from_rgb(23, 33, 46)
            };
            let v = &mut style.visuals;
            v.panel_fill = panel;
            v.window_fill = panel;
            v.faint_bg_color = elevated;
            v.extreme_bg_color = if dark {
                Color32::from_rgb(0x14, 0x19, 0x1f)
            } else {
                Color32::WHITE
            };
            v.override_text_color = Some(text);
            v.weak_text_color = Some(if dark {
                Color32::from_rgb(0xa7, 0xb2, 0xbf)
            } else {
                Color32::from_rgb(85, 100, 118)
            });
            v.window_corner_radius = CornerRadius::same(6);
            v.window_stroke = Stroke::new(1.0, border);
            v.selection.bg_fill = if dark {
                Color32::from_rgb(0x17, 0x3c, 0x67)
            } else {
                Color32::from_rgb(219, 234, 254)
            };
            v.selection.stroke = Stroke::new(1.0, ACCENT);
            v.hyperlink_color = if dark {
                ACCENT
            } else {
                Color32::from_rgb(37, 99, 235)
            };
            v.warn_fg_color = if dark {
                WARNING
            } else {
                Color32::from_rgb(138, 90, 0)
            };
            v.error_fg_color = if dark {
                ERROR
            } else {
                Color32::from_rgb(180, 35, 24)
            };
            for w in [
                &mut v.widgets.noninteractive,
                &mut v.widgets.inactive,
                &mut v.widgets.hovered,
                &mut v.widgets.active,
                &mut v.widgets.open,
            ] {
                w.corner_radius = CornerRadius::same(4);
                w.fg_stroke = Stroke::new(1.0, text);
                w.bg_stroke = Stroke::new(1.0, border);
            }
            v.widgets.inactive.bg_fill = elevated;
            v.widgets.inactive.weak_bg_fill = elevated;
            v.widgets.hovered.bg_fill = if dark {
                Color32::from_rgb(0x30, 0x3a, 0x47)
            } else {
                Color32::from_rgb(226, 232, 240)
            };
            v.widgets.hovered.weak_bg_fill = v.widgets.hovered.bg_fill;
            v.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT.gamma_multiply(0.6));
            v.widgets.active.bg_fill = v.selection.bg_fill;
            v.widgets.active.weak_bg_fill = v.selection.bg_fill;
            v.widgets.active.bg_stroke = Stroke::new(1.5, ACCENT);
            v.widgets.open.bg_fill = v.selection.bg_fill;
            v.widgets.open.weak_bg_fill = v.selection.bg_fill;
            v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, border);
            style
                .text_styles
                .insert(egui::TextStyle::Body, FontId::proportional(13.0));
            style
                .text_styles
                .insert(egui::TextStyle::Button, FontId::proportional(13.0));
            style
                .text_styles
                .insert(egui::TextStyle::Small, FontId::proportional(12.0));
            style
                .text_styles
                .insert(egui::TextStyle::Heading, FontId::proportional(15.0));
            style.spacing.item_spacing = Vec2::new(8.0, 6.0);
            style.spacing.button_padding = Vec2::new(8.0, 4.0);
            style.spacing.interact_size.y = 28.0;
            style.spacing.combo_width = 120.0;
        });
    }
    apply(ctx, WorkspaceTheme::Dark);
}

pub fn apply(ctx: &egui::Context, theme: WorkspaceTheme) {
    ctx.set_theme(match theme {
        WorkspaceTheme::Dark => Theme::Dark,
        WorkspaceTheme::Light => Theme::Light,
    });
}
pub fn grid(background: bp_model::Color) -> Color32 {
    if crate::presets::dark_background(background) {
        Color32::from_rgb(0x2e, 0x37, 0x42)
    } else {
        Color32::from_gray(228)
    }
}
pub fn fonts_ready(ctx: &egui::Context) -> bool {
    bp_render_egui::bundled_fonts_ready(ctx)
        && ctx.fonts(|f| f.families().contains(&FontFamily::Name(ICON_FAMILY.into())))
}
pub fn icon(glyph: &str) -> RichText {
    RichText::new(glyph).family(FontFamily::Name(ICON_FAMILY.into()))
}
pub fn labelled(glyph: &str, label: &str) -> (RichText, String) {
    (icon(glyph), label.to_owned())
}
