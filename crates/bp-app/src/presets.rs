//! Authored colors for new content. Loading and copying never apply presets.
use bp_commands::Command;
use bp_model::{Color, Element, ElementKind, Paint};
pub const DARK_CANVAS: Color = Color::rgb(0x15, 0x1b, 0x22);
pub fn dark_background(background: Color) -> bool {
    (0.2126 * f64::from(background.r)
        + 0.7152 * f64::from(background.g)
        + 0.0722 * f64::from(background.b))
        < 128.0
}
pub fn apply(element: &mut Element, background: Color) {
    if !dark_background(background) {
        return;
    }
    let text = Color::rgb(0xee, 0xf2, 0xf6);
    let stroke = Color::rgb(0xb8, 0xc4, 0xd3);
    match &mut element.kind {
        ElementKind::Shape(shape) => {
            shape.style.text_color = Some(text);
            if shape.shape.is_cloud() {
                return;
            }
            let (fill, outline) = match shape.shape.shape() {
                "terminator" => (Color::rgb(0x18, 0x4e, 0x4c), Color::rgb(0x54, 0xd3, 0xbc)),
                "decision" => (Color::rgb(0x63, 0x4d, 0x1c), Color::rgb(0xf0, 0xcc, 0x62)),
                "sticky-note" | "note" => {
                    shape.style.text_color = Some(Color::INK);
                    (Color::rgb(0xe7, 0xc7, 0x5b), Color::rgb(0xf0, 0xcc, 0x62))
                }
                "table" => (Color::rgb(0x1d, 0x27, 0x33), Color::rgb(0x75, 0x84, 0x97)),
                _ => (Color::rgb(0x27, 0x35, 0x48), stroke),
            };
            if !matches!(shape.shape.shape(), "text" | "annotation" | "schema") {
                shape.style.fill = Some(Paint::Color(fill));
            }
            if shape.shape.shape() != "text" {
                shape.style.stroke = Some(Paint::Color(outline));
            }
        }
        ElementKind::Connector(connector) => {
            connector.style.stroke = Some(Paint::Color(stroke));
            connector.style.text_color = Some(text);
        }
        ElementKind::Group => {}
    }
}
pub fn commands(commands: &mut [Command], background: Color) {
    for command in commands {
        if let Command::Insert(element) = command {
            apply(element, background);
        }
    }
}
