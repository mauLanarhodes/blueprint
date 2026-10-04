//! Placing laid-out text in a box.

use crate::{PlacedLine, TextRun};
use bp_model::kurbo::Rect;
use bp_model::{Color, TextAlign, VerticalAlign};
use bp_text::{Face, layout};

/// Space between a text box's edge and its text, in page units.
pub const PADDING_X: f64 = 6.0;
pub const PADDING_Y: f64 = 4.0;

pub struct TextStyle {
    pub face: Face,
    pub size: f64,
    pub color: Color,
    pub align: TextAlign,
    pub vertical_align: VerticalAlign,
}

/// Lays `text` out to fit the width of `area` (less padding) and places it
/// by the alignments. Text taller than the area overflows it evenly, as in
/// other diagram tools. Returns the run and the box its lines cover.
pub fn place(text: &str, area: Rect, style: &TextStyle, wrap: bool) -> Option<(TextRun, Rect)> {
    if text.trim().is_empty() {
        return None;
    }
    let pad_x = PADDING_X.min(area.width() / 4.0);
    let pad_y = PADDING_Y.min(area.height() / 4.0);
    let inner = area.inset((-pad_x, -pad_y));
    let max_width = wrap.then(|| inner.width().max(1.0));
    let lay = layout(text, style.face, style.size, max_width);
    let top = match style.vertical_align {
        VerticalAlign::Top => inner.y0,
        VerticalAlign::Middle => inner.center().y - lay.height / 2.0,
        VerticalAlign::Bottom => inner.y1 - lay.height,
    };
    let anchor = match style.align {
        TextAlign::Left => inner.x0,
        TextAlign::Center => inner.center().x,
        TextAlign::Right => inner.x1,
    };
    let mut covered: Option<Rect> = None;
    let lines: Vec<PlacedLine> = lay
        .lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let line_top = top + i as f64 * lay.line_height;
            let x0 = match style.align {
                TextAlign::Left => anchor,
                TextAlign::Center => anchor - line.width / 2.0,
                TextAlign::Right => anchor - line.width,
            };
            let r = Rect::new(x0, line_top, x0 + line.width, line_top + lay.line_height);
            covered = Some(covered.map_or(r, |c| c.union(r)));
            PlacedLine {
                text: line.text.clone(),
                x: anchor,
                baseline: line_top + lay.baseline,
                width: line.width,
            }
        })
        .collect();
    let covered = covered?;
    Some((
        TextRun {
            lines,
            face: style.face,
            size: style.size,
            color: style.color,
            align: style.align,
        },
        covered,
    ))
}
