//! Text measurement and line breaking with the bundled fonts.
//!
//! The screen and every exporter take their line breaks from here, so text
//! wraps the same everywhere: no "looked fine on screen, broken in the
//! PDF". Widths come from shaping with harfrust over the bundled Inter
//! faces, the same shaper egui uses to draw them.

use harfrust::{ShapeOptions, ShaperData, UnicodeBuffer};
use skrifa::instance::{LocationRef, Size};
use skrifa::{FontRef, MetadataProvider};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// The family name of the bundled UI and diagram font.
pub const FAMILY: &str = "Inter";

/// Line height as a multiple of the font size.
pub const LINE_HEIGHT: f64 = 1.25;

const FONT_DATA: [&[u8]; 4] = [
    include_bytes!("../fonts/Inter-Regular.ttf"),
    include_bytes!("../fonts/Inter-Bold.ttf"),
    include_bytes!("../fonts/Inter-Italic.ttf"),
    include_bytes!("../fonts/Inter-BoldItalic.ttf"),
];

/// One of the bundled font faces.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Face {
    Regular,
    Bold,
    Italic,
    BoldItalic,
}

impl Face {
    pub const ALL: [Face; 4] = [Face::Regular, Face::Bold, Face::Italic, Face::BoldItalic];

    pub fn new(bold: bool, italic: bool) -> Face {
        match (bold, italic) {
            (false, false) => Face::Regular,
            (true, false) => Face::Bold,
            (false, true) => Face::Italic,
            (true, true) => Face::BoldItalic,
        }
    }

    fn index(self) -> usize {
        self as usize
    }

    /// The TTF data, for registering with egui or embedding in exports.
    pub fn data(self) -> &'static [u8] {
        FONT_DATA[self.index()]
    }

    /// A unique name for this face, such as `Inter-Bold`.
    pub fn name(self) -> &'static str {
        [
            "Inter-Regular",
            "Inter-Bold",
            "Inter-Italic",
            "Inter-BoldItalic",
        ][self.index()]
    }

    pub fn is_bold(self) -> bool {
        matches!(self, Face::Bold | Face::BoldItalic)
    }

    pub fn is_italic(self) -> bool {
        matches!(self, Face::Italic | Face::BoldItalic)
    }
}

struct LoadedFace {
    font: FontRef<'static>,
    shaper: ShaperData,
    units_per_em: f64,
    /// In font units; descent is negative.
    ascent: f64,
    descent: f64,
}

fn faces() -> &'static [LoadedFace] {
    static FACES: OnceLock<Vec<LoadedFace>> = OnceLock::new();
    FACES.get_or_init(|| {
        FONT_DATA
            .iter()
            .map(|data| {
                let font = FontRef::new(data).expect("bundled fonts are valid");
                let metrics = font.metrics(Size::unscaled(), LocationRef::default());
                LoadedFace {
                    shaper: ShaperData::new(&font),
                    units_per_em: f64::from(metrics.units_per_em),
                    ascent: f64::from(metrics.ascent),
                    descent: f64::from(metrics.descent),
                    font,
                }
            })
            .collect()
    })
}

/// Widths of shaped words in font units, shared by every layout.
fn width_cache() -> &'static Mutex<HashMap<(Face, String), f64>> {
    static CACHE: OnceLock<Mutex<HashMap<(Face, String), f64>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// The advance width of `text` in font units, shaped as one run.
fn shaped_units(text: &str, face: Face) -> f64 {
    if text.is_empty() {
        return 0.0;
    }
    let key = (face, text.to_owned());
    if let Some(w) = width_cache().lock().ok().and_then(|c| c.get(&key).copied()) {
        return w;
    }
    let loaded = &faces()[face.index()];
    let shaper = loaded.shaper.shaper(&loaded.font).build();
    let mut buffer = UnicodeBuffer::new();
    buffer.push_str(text);
    buffer.guess_segment_properties();
    let glyphs = shaper.shape(buffer, ShapeOptions::new());
    let units: f64 = glyphs
        .glyph_positions()
        .iter()
        .map(|p| f64::from(p.x_advance))
        .sum();
    if let Ok(mut cache) = width_cache().lock() {
        if cache.len() > 50_000 {
            cache.clear();
        }
        cache.insert(key, units);
    }
    units
}

/// The width of `text` set on one line at `size` (in page units).
pub fn measure(text: &str, face: Face, size: f64) -> f64 {
    shaped_units(text, face) * size / faces()[face.index()].units_per_em
}

/// Vertical metrics at a size, in page units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FontMetrics {
    /// From the baseline up to the top of tall glyphs.
    pub ascent: f64,
    /// From the baseline down (positive).
    pub descent: f64,
}

pub fn metrics(face: Face, size: f64) -> FontMetrics {
    let f = &faces()[face.index()];
    FontMetrics {
        ascent: f.ascent * size / f.units_per_em,
        descent: -f.descent * size / f.units_per_em,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub text: String,
    pub width: f64,
}

/// Text broken into lines for one face and size.
#[derive(Clone, Debug, PartialEq)]
pub struct TextLayout {
    pub lines: Vec<Line>,
    pub face: Face,
    pub size: f64,
    pub line_height: f64,
    /// From the top of a line box to its baseline.
    pub baseline: f64,
    /// The widest line.
    pub width: f64,
    /// All lines stacked.
    pub height: f64,
}

/// Breaks `text` into lines no wider than `max_width` (when given),
/// breaking after spaces and hyphens. A word wider than a whole line gets
/// a line to itself and overflows it rather than being split, as in CSS
/// and other diagram tools. Explicit newlines always break.
pub fn layout(text: &str, face: Face, size: f64, max_width: Option<f64>) -> TextLayout {
    let size = size.max(0.1);
    let line_height = size * LINE_HEIGHT;
    let m = metrics(face, size);
    let baseline = (line_height - (m.ascent + m.descent)) / 2.0 + m.ascent;
    let mut lines = Vec::new();
    if !text.is_empty() {
        let text = text.replace('\t', "    ");
        for paragraph in text.split('\n') {
            let paragraph = paragraph.trim_end_matches('\r');
            match max_width {
                Some(max) if max > 0.0 => wrap(paragraph, face, size, max, &mut lines),
                _ => lines.push(Line {
                    text: paragraph.trim_end().to_owned(),
                    width: measure(paragraph.trim_end(), face, size),
                }),
            }
        }
    }
    let width = lines.iter().map(|l| l.width).fold(0.0, f64::max);
    let height = lines.len() as f64 * line_height;
    TextLayout {
        lines,
        face,
        size,
        line_height,
        baseline,
        width,
        height,
    }
}

/// A piece of a paragraph that ends at a break opportunity.
struct Piece<'a> {
    text: &'a str,
    /// With trailing spaces.
    full: f64,
    /// Without trailing spaces: the width when the piece ends a line.
    trimmed: f64,
}

fn pieces(paragraph: &str, face: Face, size: f64) -> Vec<Piece<'_>> {
    let mut out = Vec::new();
    let mut start = 0;
    let chars: Vec<(usize, char)> = paragraph.char_indices().collect();
    for (k, &(i, c)) in chars.iter().enumerate() {
        let next = chars.get(k + 1).map(|&(_, n)| n);
        let end = i + c.len_utf8();
        let break_after = match next {
            None => true,
            Some(n) => {
                (c.is_whitespace() && !n.is_whitespace()) || (c == '-' && n.is_alphanumeric())
            }
        };
        if break_after {
            let text = &paragraph[start..end];
            let trimmed_text = text.trim_end();
            out.push(Piece {
                text,
                full: measure(text, face, size),
                trimmed: measure(trimmed_text, face, size),
            });
            start = end;
        }
    }
    out
}

fn wrap(paragraph: &str, face: Face, size: f64, max: f64, lines: &mut Vec<Line>) {
    let mut current = String::new();
    // Width of `current` if it ended here, and with its trailing spaces.
    let mut trimmed = 0.0;
    let mut full = 0.0;
    let push = |lines: &mut Vec<Line>, text: &str, width: f64| {
        lines.push(Line {
            text: text.trim_end().to_owned(),
            width,
        });
    };
    let pieces = pieces(paragraph, face, size);
    if pieces.is_empty() {
        push(lines, "", 0.0);
        return;
    }
    for piece in pieces {
        if !current.is_empty() && full + piece.trimmed > max {
            push(lines, &current, trimmed);
            current.clear();
            full = 0.0;
        }
        current.push_str(piece.text);
        trimmed = full + piece.trimmed;
        full += piece.full;
    }
    if !current.is_empty() {
        push(lines, &current, trimmed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faces_load_with_sane_metrics() {
        for face in Face::ALL {
            let m = metrics(face, 100.0);
            assert!(m.ascent > 80.0 && m.ascent < 110.0, "{face:?} {m:?}");
            assert!(m.descent > 15.0 && m.descent < 35.0, "{face:?} {m:?}");
        }
        assert_eq!(Face::new(true, true), Face::BoldItalic);
    }

    #[test]
    fn widths_scale_with_size_and_weight() {
        let w14 = measure("Orders API", Face::Regular, 14.0);
        let w28 = measure("Orders API", Face::Regular, 28.0);
        assert!(w14 > 50.0 && w14 < 90.0, "{w14}");
        assert!((w28 - 2.0 * w14).abs() < 1e-9);
        assert!(measure("Orders API", Face::Bold, 14.0) > w14);
        assert_eq!(measure("", Face::Regular, 14.0), 0.0);
    }

    #[test]
    fn kerning_is_applied() {
        // "AV" kerns tighter than its two letters apart.
        let pair = measure("AV", Face::Regular, 100.0);
        let apart = measure("A", Face::Regular, 100.0) + measure("V", Face::Regular, 100.0);
        assert!(pair < apart, "{pair} vs {apart}");
    }

    #[test]
    fn wraps_at_spaces() {
        let text = "Validate the order and reserve stock";
        let l = layout(text, Face::Regular, 14.0, Some(120.0));
        assert!(l.lines.len() >= 2, "{:?}", l.lines);
        for line in &l.lines {
            assert!(line.width <= 120.0, "{line:?}");
            assert!(!line.text.ends_with(' '));
        }
        let joined: Vec<_> = l.lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(joined.join(" "), text);
        assert_eq!(l.height, l.lines.len() as f64 * 14.0 * LINE_HEIGHT);
    }

    #[test]
    fn long_words_overflow_and_blank_lines_stay() {
        let l = layout(
            "an Extraordinarily long\n\nok",
            Face::Regular,
            14.0,
            Some(40.0),
        );
        let texts: Vec<_> = l.lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, ["an", "Extraordinarily", "long", "", "ok"]);
        assert!(l.lines[1].width > 40.0, "the long word overflows");
    }

    #[test]
    fn hyphens_are_break_points() {
        let l = layout(
            "well-known",
            Face::Regular,
            14.0,
            Some(measure("well-kn", Face::Regular, 14.0)),
        );
        assert_eq!(
            l.lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(),
            ["well-", "known"]
        );
    }

    #[test]
    fn no_width_means_one_line_per_paragraph() {
        let l = layout("a b c\nd", Face::Regular, 14.0, None);
        assert_eq!(l.lines.len(), 2);
        assert!(layout("", Face::Regular, 14.0, Some(10.0)).lines.is_empty());
    }

    #[test]
    fn baseline_centres_the_glyphs_in_the_line() {
        let l = layout("x", Face::Regular, 20.0, None);
        let m = metrics(Face::Regular, 20.0);
        let above = l.baseline - m.ascent;
        let below = l.line_height - l.baseline - m.descent;
        assert!((above - below).abs() < 1e-9);
    }
}
