use serde::{Deserialize, Serialize};
use std::fmt;

/// An sRGB colour with straight (not premultiplied) alpha.
///
/// Stored in files as `#rrggbb` or `#rrggbbaa` so diffs stay readable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const BLACK: Color = Color::rgb(0, 0, 0);
    pub const WHITE: Color = Color::rgb(255, 255, 255);
    pub const INK: Color = Color::rgb(0x11, 0x18, 0x27);
    pub const OUTLINE: Color = Color::rgb(0x1f, 0x29, 0x37);

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub fn is_opaque(&self) -> bool {
        self.a == 255
    }

    /// `#rrggbb` (ignores alpha), for SVG `fill` and `stroke` attributes.
    pub fn to_hex_rgb(&self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    /// Alpha as 0.0–1.0, for SVG `fill-opacity` and `stroke-opacity`.
    pub fn opacity(&self) -> f64 {
        f64::from(self.a) / 255.0
    }

    /// The same colour with its alpha multiplied by `opacity` (0.0–1.0).
    pub fn faded(self, opacity: f64) -> Self {
        let a = (f64::from(self.a) * opacity.clamp(0.0, 1.0)).round() as u8;
        Self { a, ..self }
    }

    /// Parses `#rgb`, `#rrggbb` or `#rrggbbaa`.
    pub fn parse_hex(s: &str) -> Option<Self> {
        let hex = s.strip_prefix('#')?;
        let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
        match hex.len() {
            3 => {
                let mut c = hex.chars().map(|c| c.to_digit(16).map(|v| (v * 17) as u8));
                Some(Self::rgb(c.next()??, c.next()??, c.next()??))
            }
            6 => Some(Self::rgb(byte(0)?, byte(2)?, byte(4)?)),
            8 => Some(Self::rgba(byte(0)?, byte(2)?, byte(4)?, byte(6)?)),
            _ => None,
        }
    }
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_opaque() {
            f.write_str(&self.to_hex_rgb())
        } else {
            write!(f, "{}{:02x}", self.to_hex_rgb(), self.a)
        }
    }
}

impl From<Color> for String {
    fn from(c: Color) -> String {
        c.to_string()
    }
}

impl TryFrom<String> for Color {
    type Error = String;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Color::parse_hex(&s).ok_or_else(|| format!("invalid colour {s:?}"))
    }
}

/// A fill or stroke: a colour, or explicitly nothing. Stored as `"none"` or
/// a hex colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub enum Paint {
    None,
    Color(Color),
}

impl Paint {
    pub fn color(self) -> Option<Color> {
        match self {
            Paint::None => None,
            Paint::Color(c) => Some(c),
        }
    }
}

impl From<Option<Color>> for Paint {
    fn from(c: Option<Color>) -> Self {
        c.map_or(Paint::None, Paint::Color)
    }
}

impl From<Paint> for String {
    fn from(p: Paint) -> String {
        match p {
            Paint::None => "none".into(),
            Paint::Color(c) => c.to_string(),
        }
    }
}

impl TryFrom<String> for Paint {
    type Error = String;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        if s == "none" {
            Ok(Paint::None)
        } else {
            Color::try_from(s).map(Paint::Color)
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Dash {
    #[default]
    Solid,
    Dashed,
    Dotted,
}

impl Dash {
    pub const ALL: [Dash; 3] = [Dash::Solid, Dash::Dashed, Dash::Dotted];

    /// The on/off lengths for a stroke of `width`, or `None` for solid.
    pub fn pattern(self, width: f64) -> Option<[f64; 2]> {
        let w = width.max(0.5);
        match self {
            Dash::Solid => None,
            Dash::Dashed => Some([4.0 * w, 3.0 * w]),
            Dash::Dotted => Some([w, 2.0 * w]),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Dash::Solid => "Solid",
            Dash::Dashed => "Dashed",
            Dash::Dotted => "Dotted",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextAlign {
    Left,
    #[default]
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerticalAlign {
    Top,
    #[default]
    Middle,
    Bottom,
}

/// The style overrides of one element. Every field is optional: `None`
/// inherits from the shape's defaults, so a file stores only what the user
/// changed, and each field is set by its own command.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Style {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill: Option<Paint>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stroke: Option<Paint>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stroke_width: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dash: Option<Dash>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shadow: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub corner_radius: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_color: Option<Color>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_size: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bold: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_align: Option<TextAlign>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vertical_align: Option<VerticalAlign>,
}

impl Style {
    pub fn is_empty(&self) -> bool {
        *self == Style::default()
    }

    /// `base` with every override in `self` applied.
    pub fn resolve(&self, base: &StyleValues) -> StyleValues {
        StyleValues {
            fill: self.fill.map_or(base.fill, Paint::color),
            stroke: self.stroke.map_or(base.stroke, Paint::color),
            stroke_width: self.stroke_width.unwrap_or(base.stroke_width).max(0.0),
            dash: self.dash.unwrap_or(base.dash),
            opacity: self.opacity.unwrap_or(base.opacity).clamp(0.0, 1.0),
            shadow: self.shadow.unwrap_or(base.shadow),
            corner_radius: self.corner_radius.unwrap_or(base.corner_radius).max(0.0),
            text_color: self.text_color.unwrap_or(base.text_color),
            font_size: self.font_size.unwrap_or(base.font_size).max(1.0),
            bold: self.bold.unwrap_or(base.bold),
            italic: self.italic.unwrap_or(base.italic),
            text_align: self.text_align.unwrap_or(base.text_align),
            vertical_align: self.vertical_align.unwrap_or(base.vertical_align),
        }
    }
}

/// A fully resolved style: what the scene builder actually draws with.
#[derive(Clone, Debug, PartialEq)]
pub struct StyleValues {
    pub fill: Option<Color>,
    pub stroke: Option<Color>,
    pub stroke_width: f64,
    pub dash: Dash,
    pub opacity: f64,
    pub shadow: bool,
    pub corner_radius: f64,
    pub text_color: Color,
    pub font_size: f64,
    pub bold: bool,
    pub italic: bool,
    pub text_align: TextAlign,
    pub vertical_align: VerticalAlign,
}

impl Default for StyleValues {
    fn default() -> Self {
        Self {
            fill: Some(Color::WHITE),
            stroke: Some(Color::OUTLINE),
            stroke_width: 1.5,
            dash: Dash::Solid,
            opacity: 1.0,
            shadow: false,
            corner_radius: 0.0,
            text_color: Color::INK,
            font_size: 14.0,
            bold: false,
            italic: false,
            text_align: TextAlign::Center,
            vertical_align: VerticalAlign::Middle,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trip() {
        for s in ["#1f2937", "#ffffff80"] {
            assert_eq!(Color::parse_hex(s).unwrap().to_string(), s);
        }
        assert_eq!(Color::parse_hex("#fff"), Some(Color::WHITE));
        assert_eq!(Color::parse_hex("red"), None);
        assert_eq!(Color::parse_hex("#12345"), None);
    }

    #[test]
    fn serialises_as_hex_string() {
        let json = serde_json::to_string(&Color::rgb(255, 0, 16)).unwrap();
        assert_eq!(json, "\"#ff0010\"");
        let back: Color = serde_json::from_str(&json).unwrap();
        assert_eq!(back, Color::rgb(255, 0, 16));
    }

    #[test]
    fn paint_is_none_or_a_colour() {
        let json = serde_json::to_string(&[Paint::None, Paint::Color(Color::WHITE)]).unwrap();
        assert_eq!(json, r##"["none","#ffffff"]"##);
        let back: Vec<Paint> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, [Paint::None, Paint::Color(Color::WHITE)]);
        assert!(serde_json::from_str::<Paint>("\"nope\"").is_err());
    }

    #[test]
    fn only_overrides_are_stored() {
        assert_eq!(serde_json::to_string(&Style::default()).unwrap(), "{}");
        let style = Style {
            fill: Some(Paint::None),
            font_size: Some(18.0),
            ..Style::default()
        };
        let json = serde_json::to_string(&style).unwrap();
        assert_eq!(json, r#"{"fill":"none","font_size":18.0}"#);
        assert_eq!(serde_json::from_str::<Style>(&json).unwrap(), style);
    }

    #[test]
    fn resolve_applies_overrides_and_clamps() {
        let base = StyleValues::default();
        let style = Style {
            fill: Some(Paint::None),
            opacity: Some(7.0),
            ..Style::default()
        };
        let v = style.resolve(&base);
        assert_eq!(v.fill, None);
        assert_eq!(v.stroke, base.stroke);
        assert_eq!(v.opacity, 1.0);
    }

    #[test]
    fn faded_scales_alpha() {
        assert_eq!(Color::WHITE.faded(0.5).a, 128);
        assert_eq!(Color::rgba(0, 0, 0, 100).faded(2.0).a, 100);
    }
}
