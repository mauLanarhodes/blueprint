//! Snapping while dragging: smart guides to other shapes' edges and
//! centres, falling back to the grid.

use kurbo::{Point, Rect, Vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    /// A vertical guide at some x.
    X,
    /// A horizontal guide at some y.
    Y,
}

/// A guide line to draw: `at` on `axis`, spanning `from`..`to` on the
/// other axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Guide {
    pub axis: Axis,
    pub at: f64,
    pub from: f64,
    pub to: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SnapResult {
    /// Add this to the dragged position.
    pub delta: Vec2,
    pub guides: Vec<Guide>,
}

/// Which features of the moving box may snap: `[start, centre, end]` on
/// each axis. A move uses all six; resizing from a corner uses that
/// corner's two edges.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Features {
    pub x: [bool; 3],
    pub y: [bool; 3],
}

impl Features {
    pub const ALL: Features = Features {
        x: [true; 3],
        y: [true; 3],
    };
    pub const CENTRE: Features = Features {
        x: [false, true, false],
        y: [false, true, false],
    };
}

fn xs(r: &Rect) -> [f64; 3] {
    [r.x0, r.center().x, r.x1]
}

fn ys(r: &Rect) -> [f64; 3] {
    [r.y0, r.center().y, r.y1]
}

/// The correction for one axis: to the nearest target feature within
/// `tolerance`, otherwise the first enabled feature to the grid.
fn snap_axis(
    moving: [f64; 3],
    enabled: [bool; 3],
    targets: &[[f64; 3]],
    grid: Option<f64>,
    tolerance: f64,
) -> f64 {
    let mut best: Option<f64> = None;
    for (m, _) in moving.iter().zip(enabled).filter(|(_, on)| *on) {
        for t in targets.iter().flatten() {
            let d = t - m;
            if d.abs() <= tolerance && best.is_none_or(|b| d.abs() < b.abs()) {
                best = Some(d);
            }
        }
    }
    if let Some(d) = best {
        return d;
    }
    match (grid, enabled.iter().position(|on| *on)) {
        (Some(g), Some(i)) if g > 0.0 => (moving[i] / g).round() * g - moving[i],
        _ => 0.0,
    }
}

/// Snaps `moving` (the box as dragged) against `targets`.
pub fn snap_rect(
    moving: Rect,
    features: Features,
    targets: &[Rect],
    grid: Option<f64>,
    tolerance: f64,
) -> SnapResult {
    let target_xs: Vec<[f64; 3]> = targets.iter().map(xs).collect();
    let target_ys: Vec<[f64; 3]> = targets.iter().map(ys).collect();
    let dx = snap_axis(xs(&moving), features.x, &target_xs, grid, tolerance);
    let dy = snap_axis(ys(&moving), features.y, &target_ys, grid, tolerance);
    let delta = Vec2::new(dx, dy);
    let snapped = moving + delta;
    SnapResult {
        delta,
        guides: guides(&snapped, features, targets),
    }
}

/// Snaps a single point (a connector end or waypoint) to targets' centres
/// and edges, or the grid.
pub fn snap_point(p: Point, targets: &[Rect], grid: Option<f64>, tolerance: f64) -> SnapResult {
    let moving = Rect::from_points(p, p);
    let features = Features {
        x: [true, false, false],
        y: [true, false, false],
    };
    snap_rect(moving, features, targets, grid, tolerance)
}

/// A guide for every enabled feature of `snapped` that lines up exactly
/// with a target feature, spanning everything aligned on it.
fn guides(snapped: &Rect, features: Features, targets: &[Rect]) -> Vec<Guide> {
    let mut out: Vec<Guide> = Vec::new();
    let mut add = |axis: Axis, at: f64, from: f64, to: f64| {
        if let Some(g) = out
            .iter_mut()
            .find(|g| g.axis == axis && (g.at - at).abs() < 1e-6)
        {
            g.from = g.from.min(from);
            g.to = g.to.max(to);
        } else {
            out.push(Guide { axis, at, from, to });
        }
    };
    for t in targets {
        for (m, on) in xs(snapped).into_iter().zip(features.x) {
            if on && xs(t).iter().any(|v| (v - m).abs() < 1e-6) {
                add(Axis::X, m, snapped.y0.min(t.y0), snapped.y1.max(t.y1));
            }
        }
        for (m, on) in ys(snapped).into_iter().zip(features.y) {
            if on && ys(t).iter().any(|v| (v - m).abs() < 1e-6) {
                add(Axis::Y, m, snapped.x0.min(t.x0), snapped.x1.max(t.x1));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snaps_to_a_nearby_edge_and_draws_a_guide() {
        let target = Rect::new(100.0, 0.0, 200.0, 50.0);
        // Left edge 3 units right of the target's left edge.
        let moving = Rect::new(103.0, 200.0, 143.0, 230.0);
        let r = snap_rect(moving, Features::ALL, &[target], Some(10.0), 5.0);
        assert_eq!(r.delta.x, -3.0);
        assert_eq!(r.delta.y, 0.0, "y falls back to the grid: 200 is on it");
        assert_eq!(
            r.guides,
            vec![Guide {
                axis: Axis::X,
                at: 100.0,
                from: 0.0,
                to: 230.0
            }]
        );
    }

    #[test]
    fn centres_snap_too() {
        let target = Rect::new(0.0, 0.0, 100.0, 100.0);
        let moving = Rect::new(36.0, 300.0, 66.0, 330.0); // centre x = 51
        let r = snap_rect(moving, Features::ALL, &[target], None, 4.0);
        assert_eq!(r.delta.x, -1.0);
        assert!(r.guides.iter().any(|g| g.axis == Axis::X && g.at == 50.0));
    }

    #[test]
    fn grid_snaps_the_top_left_corner() {
        let r = snap_rect(
            Rect::new(13.0, 27.0, 50.0, 50.0),
            Features::ALL,
            &[],
            Some(10.0),
            5.0,
        );
        assert_eq!(r.delta, Vec2::new(-3.0, 3.0));
        assert!(r.guides.is_empty());
    }

    #[test]
    fn resizing_snaps_only_the_dragged_edges() {
        let target = Rect::new(0.0, 0.0, 100.0, 100.0);
        // Dragging the right edge to x = 98: the left edge (at 0) must not
        // count, even though it is aligned already.
        let moving = Rect::new(0.0, 200.0, 98.0, 260.0);
        let features = Features {
            x: [false, false, true],
            y: [false, false, false],
        };
        let r = snap_rect(moving, features, &[target], None, 5.0);
        assert_eq!(r.delta, Vec2::new(2.0, 0.0));
    }

    #[test]
    fn points_snap_to_edges() {
        let target = Rect::new(100.0, 100.0, 200.0, 200.0);
        let r = snap_point(Point::new(98.0, 151.0), &[target], None, 5.0);
        assert_eq!(r.delta, Vec2::new(2.0, -1.0));
    }
}
