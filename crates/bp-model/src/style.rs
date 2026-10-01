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

/// Visual style of one element. Every field is set individually by
/// commands, so concurrent edits to different fields never clash.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Style {
    pub fill: Option<Color>,
    pub stroke: Option<Color>,
    pub stroke_width: f64,
    pub text_color: Color,
    pub font_size: f64,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            fill: Some(Color::WHITE),
            stroke: Some(Color::OUTLINE),
            stroke_width: 1.5,
            text_color: Color::INK,
            font_size: 14.0,
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
}