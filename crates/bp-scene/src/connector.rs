//! Connector routes, end markers and labels.

use crate::{ShapeGeometry, Stroke};
use bp_geom::{Dir, Terminal, polyline_point_at, ray_exit, route_orthogonal, simplify_polyline};
use bp_model::kurbo::{
    BezPath, ParamCurve, ParamCurveArclen, ParamCurveDeriv, PathEl, PathSeg, Point, Shape, Vec2,
};
use bp_model::{Color, Connector, ElementId, Endpoint, Marker, Routing};

/// Clearance kept around shapes by orthogonal routes, and the length of
/// the straight stub leaving each port.
pub const ROUTE_MARGIN: f64 = 20.0;

/// How one end of a connector resolved against the document.
#[derive(Clone, Debug)]
pub(crate) enum End<'a> {
    Free(Point),
    /// At a fixed port.
    Port {
        at: Point,
        dir: Dir,
        bounds: bp_model::kurbo::Rect,
    },
    /// Floating on a shape's outline: the route picks the point.
    Floating {
        shape: &'a ShapeGeometry,
    },
}

impl<'a> End<'a> {
    pub(crate) fn resolve(
        end: &Endpoint,
        shape: impl Fn(ElementId) -> Option<&'a ShapeGeometry>,
    ) -> End<'a> {
        match end {
            Endpoint::Free(p) => End::Free(*p),
            Endpoint::Glued { element, port } => {
                let Some(g) = shape(*element) else {
                    // Validation keeps this from happening; draw something
                    // findable rather than nothing.
                    return End::Free(Point::ZERO);
                };
                match port
                    .as_ref()
                    .and_then(|id| g.ports.iter().find(|p| &p.id == id))
                {
                    Some(p) => End::Port {
                        at: p.at,
                        dir: p.dir,
                        bounds: g.bounds,
                    },
                    None => End::Floating { shape: g },
                }
            }
        }
    }

    /// A point that stands for this end when aiming the other end at it.
    fn reference(&self) -> Point {
        match self {
            End::Free(p) => *p,
            End::Port { at, .. } => *at,
            End::Floating { shape } => shape.bounds.center(),
        }
    }

    /// The candidate terminals for the orthogonal router.
    fn terminals(&self) -> Vec<Terminal> {
        match self {
            End::Free(p) => vec![Terminal::free(*p)],
            End::Port { at, dir, bounds } => vec![Terminal {
                point: *at,
                dir: Some(*dir),
                obstacle: Some(*bounds),
            }],
            End::Floating { shape } => shape
                .ports
                .iter()
                .map(|p| Terminal {
                    point: p.at,
                    dir: Some(p.dir),
                    obstacle: Some(shape.bounds),
                })
                .collect(),
        }
    }

    /// Where a straight line aimed at `toward` meets this end.
    fn point_toward(&self, toward: Point) -> Point {
        match self {
            End::Free(p) => *p,
            End::Port { at, .. } => *at,
            End::Floating { shape } => {
                let center = shape.bounds.center();
                let mut outline = shape.outline.clone();
                for back in &shape.back {
                    outline.extend(back.iter());
                }
                ray_exit(&outline, center, toward - center).unwrap_or(center)
            }
        }
    }

    /// The direction a curve should leave this end at `at`.
    fn leave_dir(&self, at: Point, toward: Point) -> Vec2 {
        let v = match self {
            End::Port { dir, .. } => dir.vec(),
            End::Floating { shape } => {
                Dir::from_vec(at - shape.bounds.center()).map_or(toward - at, Dir::vec)
            }
            End::Free(p) => toward - *p,
        };
        let len = v.hypot();
        if len > 0.0 {
            v / len
        } else {
            Vec2::new(1.0, 0.0)
        }
    }
}

/// A connector's resolved path before markers are applied.
pub(crate) struct Route {
    pub path: BezPath,
    /// A polyline that follows the path, for hit tests and labels.
    pub points: Vec<Point>,
}

pub(crate) fn route(c: &Connector, source: &End, target: &End) -> Route {
    match c.routing {
        Routing::Orthogonal => {
            let points = orthogonal(source, target, &c.waypoints);
            Route {
                path: polyline_path(&points),
                points,
            }
        }
        Routing::Straight => {
            let points = straight_points(source, target, &c.waypoints);
            Route {
                path: polyline_path(&points),
                points,
            }
        }
        Routing::Curved => curved(source, target, &c.waypoints),
    }
}

fn orthogonal(source: &End, target: &End, waypoints: &[Point]) -> Vec<Point> {
    let mut points = match (waypoints.first(), waypoints.last()) {
        (Some(&first), Some(&last)) => {
            let mut pts = route_orthogonal(
                &source.terminals(),
                &[Terminal::free(first)],
                &[],
                ROUTE_MARGIN,
            )
            .points;
            for w in waypoints.windows(2) {
                let (a, b) = (w[0], w[1]);
                // Continue in the direction we arrived, then turn once.
                let arrived_horizontal = pts
                    .len()
                    .checked_sub(2)
                    .is_some_and(|i| (pts[i].y - pts[i + 1].y).abs() < 1e-9);
                let corner = if arrived_horizontal {
                    Point::new(b.x, a.y)
                } else {
                    Point::new(a.x, b.y)
                };
                pts.extend([corner, b]);
            }
            let tail = route_orthogonal(
                &[Terminal::free(last)],
                &target.terminals(),
                &[],
                ROUTE_MARGIN,
            )
            .points;
            pts.extend(tail.into_iter().skip(1));
            pts
        }
        _ => route_orthogonal(&source.terminals(), &target.terminals(), &[], ROUTE_MARGIN).points,
    };
    simplify_polyline(&mut points);
    points
}

fn straight_points(source: &End, target: &End, waypoints: &[Point]) -> Vec<Point> {
    let first_aim = waypoints
        .first()
        .copied()
        .unwrap_or_else(|| target.reference());
    let last_aim = waypoints
        .last()
        .copied()
        .unwrap_or_else(|| source.reference());
    let mut points = vec![source.point_toward(first_aim)];
    points.extend_from_slice(waypoints);
    points.push(target.point_toward(last_aim));
    points.dedup_by(|b, a| (*b - *a).hypot() < 1e-9);
    if points.len() == 1 {
        points.push(points[0]);
    }
    points
}

fn curved(source: &End, target: &End, waypoints: &[Point]) -> Route {
    let pts = straight_points(source, target, waypoints);
    let (start, end) = (pts[0], *pts.last().expect("two points"));
    let mut path = BezPath::new();
    path.move_to(start);
    if waypoints.is_empty() {
        let d = ((end - start).hypot() * 0.4).clamp(20.0, 150.0);
        let c1 = start + source.leave_dir(start, end) * d;
        let c2 = end + target.leave_dir(end, start) * d;
        path.curve_to(c1, c2, end);
    } else {
        // Catmull-Rom through every point, as cubic segments.
        let n = pts.len();
        for i in 0..n - 1 {
            let p0 = pts[i.saturating_sub(1)];
            let (p1, p2) = (pts[i], pts[i + 1]);
            let p3 = pts[(i + 2).min(n - 1)];
            let c1 = p1 + (p2 - p0) / 6.0;
            let c2 = p2 - (p3 - p1) / 6.0;
            path.curve_to(c1, c2, p2);
        }
    }
    let mut points = Vec::new();
    bp_model::kurbo::flatten(path.iter(), 0.25, |el| match el {
        PathEl::MoveTo(p) | PathEl::LineTo(p) => points.push(p),
        _ => {}
    });
    Route { path, points }
}

fn polyline_path(points: &[Point]) -> BezPath {
    let mut path = BezPath::new();
    if let Some((first, rest)) = points.split_first() {
        path.move_to(*first);
        for p in rest {
            path.line_to(*p);
        }
    }
    path
}

/// One path of a marker: what to draw and how.
pub(crate) struct MarkerPath {
    pub path: BezPath,
    pub fill: Option<Color>,
    pub stroke: Option<Stroke>,
}

/// The length of a marker's body along the line, for a stroke `width`.
fn marker_length(width: f64) -> f64 {
    8.0 + 2.0 * width
}

/// The shape of `marker` with its tip at `tip`, pointing along `dir`
/// (a unit vector in the direction of travel). Returns the paths to draw
/// and how far the line should stop short of the tip.
pub(crate) fn marker(
    marker: Marker,
    tip: Point,
    dir: Vec2,
    stroke: &Stroke,
    background: Color,
) -> (Vec<MarkerPath>, f64) {
    let len = marker_length(stroke.width);
    let half = len * 0.42;
    let normal = Vec2::new(-dir.y, dir.x);
    let solid = Stroke {
        dash: None,
        ..*stroke
    };
    let poly = |pts: &[Point]| {
        let mut p = polyline_path(pts);
        p.close_path();
        p
    };
    let back = tip - dir * len;
    match marker {
        Marker::None => (Vec::new(), 0.0),
        Marker::Arrow => (
            vec![MarkerPath {
                path: poly(&[tip, back + normal * half, back - normal * half]),
                fill: Some(stroke.color),
                stroke: Some(Stroke {
                    width: stroke.width.min(1.0),
                    ..solid
                }),
            }],
            len * 0.85,
        ),
        Marker::OpenArrow => (
            vec![MarkerPath {
                path: polyline_path(&[back + normal * half, tip, back - normal * half]),
                fill: None,
                stroke: Some(solid),
            }],
            stroke.width / 2.0,
        ),
        Marker::Triangle => (
            vec![MarkerPath {
                path: poly(&[tip, back + normal * half, back - normal * half]),
                fill: Some(background),
                stroke: Some(solid),
            }],
            len,
        ),
        Marker::Diamond | Marker::OpenDiamond => {
            let long = len * 1.5;
            let mid = tip - dir * (long / 2.0);
            let path = poly(&[
                tip,
                mid + normal * half,
                tip - dir * long,
                mid - normal * half,
            ]);
            let fill = if marker == Marker::Diamond {
                stroke.color
            } else {
                background
            };
            (
                vec![MarkerPath {
                    path,
                    fill: Some(fill),
                    stroke: Some(solid),
                }],
                long,
            )
        }
        Marker::Circle | Marker::OpenCircle => {
            let r = len * 0.38;
            let path = bp_model::kurbo::Circle::new(tip - dir * r, r).to_path(0.05);
            let fill = if marker == Marker::Circle {
                stroke.color
            } else {
                background
            };
            (
                vec![MarkerPath {
                    path,
                    fill: Some(fill),
                    stroke: Some(solid),
                }],
                r * 2.0,
            )
        }
        Marker::ExactlyOne
        | Marker::ZeroOrOne
        | Marker::OneOrMany
        | Marker::ZeroOrMany
        | Marker::Many => {
            // The fork touches the entity, while the minimum-cardinality
            // symbol sits farther along the relationship. Open circles
            // mask the line beneath them with the same page background.
            let at = |distance: f64| tip - dir * distance;
            let bar = |distance: f64| MarkerPath {
                path: polyline_path(&[at(distance) - normal * half, at(distance) + normal * half]),
                fill: None,
                stroke: Some(solid),
            };
            let circle = |distance: f64| MarkerPath {
                path: bp_model::kurbo::Circle::new(at(distance), len * 0.3).to_path(0.05),
                fill: Some(background),
                stroke: Some(solid),
            };
            let mut paths = Vec::new();
            if matches!(
                marker,
                Marker::OneOrMany | Marker::ZeroOrMany | Marker::Many
            ) {
                let hub = at(len);
                let mut path = BezPath::new();
                path.move_to(tip - normal * half);
                path.line_to(hub);
                path.line_to(tip + normal * half);
                paths.push(MarkerPath {
                    path,
                    fill: None,
                    stroke: Some(solid),
                });
            } else {
                paths.push(bar(len * 0.3));
            }
            match marker {
                Marker::ExactlyOne => paths.push(bar(len)),
                Marker::ZeroOrOne => paths.push(circle(len * 1.1)),
                Marker::OneOrMany => paths.push(bar(len * 1.35)),
                Marker::ZeroOrMany => paths.push(circle(len * 1.5)),
                _ => {}
            }
            // Keep the central prong and the line connecting the marker
            // to the entity; hollow symbols cover only their own area.
            (paths, 0.0)
        }
    }
}

/// The direction of travel at the start (`at_end = false`) or end of a
/// path, as a unit vector pointing out of the path at that end.
pub(crate) fn end_direction(path: &BezPath, at_end: bool) -> Option<Vec2> {
    let segs: Vec<PathSeg> = path.segments().collect();
    let seg = if at_end { segs.last()? } else { segs.first()? };
    let v = match seg {
        PathSeg::Line(l) => l.p1 - l.p0,
        PathSeg::Quad(q) => q.deriv().eval(if at_end { 1.0 } else { 0.0 }).to_vec2(),
        PathSeg::Cubic(c) => {
            let d = c.deriv().eval(if at_end { 1.0 } else { 0.0 }).to_vec2();
            if d.hypot() > 1e-9 { d } else { c.p3 - c.p0 }
        }
    };
    let v = if at_end { v } else { -v };
    let len = v.hypot();
    (len > 1e-9).then(|| v / len)
}

/// `path` with `start` and `end` units of length cut off its ends (as
/// long as something is left).
pub(crate) fn trim(path: &BezPath, start: f64, end: f64) -> BezPath {
    let segs: Vec<PathSeg> = path.segments().collect();
    if segs.is_empty() {
        return path.clone();
    }
    let lengths: Vec<f64> = segs.iter().map(|s| s.arclen(0.01)).collect();
    let from = start.max(0.0);
    let to = lengths.iter().sum::<f64>() - end.max(0.0);
    let mut out = BezPath::new();
    if from >= to {
        return out;
    }
    let mut offset = 0.0;
    for (seg, len) in segs.into_iter().zip(lengths) {
        let next = offset + len;
        if next <= from || offset >= to || len <= 0.0 {
            offset = next;
            continue;
        }
        let t0 = if from > offset {
            seg.inv_arclen(from - offset, 0.01)
        } else {
            0.0
        };
        let t1 = if to < next {
            seg.inv_arclen(to - offset, 0.01)
        } else {
            1.0
        };
        let seg = seg.subsegment(t0..t1);
        if out.is_empty() {
            out.move_to(seg.start());
        }
        match seg {
            PathSeg::Line(l) => out.line_to(l.p1),
            PathSeg::Quad(q) => out.quad_to(q.p1, q.p2),
            PathSeg::Cubic(c) => out.curve_to(c.p1, c.p2, c.p3),
        }
        offset = next;
    }
    out
}

/// The label anchor: the point at `t` along the route.
pub(crate) fn label_point(points: &[Point], t: f64) -> Point {
    polyline_point_at(points, t).map_or(Point::ZERO, |(p, _)| p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_trims_cross_short_terminal_segments() {
        let path = polyline_path(&[
            Point::new(0.0, 0.0),
            Point::new(5.0, 0.0),
            Point::new(5.0, 100.0),
            Point::new(10.0, 100.0),
        ]);
        let trimmed = trim(&path, 10.0, 10.0);
        let segments: Vec<_> = trimmed.segments().collect();
        assert_eq!(segments.first().unwrap().start(), Point::new(5.0, 5.0));
        assert_eq!(segments.last().unwrap().end(), Point::new(5.0, 95.0));
    }

    #[test]
    fn markers_covering_the_whole_route_leave_no_stroke() {
        let path = polyline_path(&[Point::ZERO, Point::new(10.0, 0.0)]);
        assert!(trim(&path, 6.0, 6.0).segments().next().is_none());
    }

    #[test]
    fn crows_foot_markers_use_solid_bars_forks_and_hollow_circles() {
        let stroke = Stroke {
            color: Color::BLACK,
            width: 1.5,
            dash: Some([4.0, 3.0]),
        };
        let tip = Point::new(100.0, 50.0);
        let len = marker_length(stroke.width);
        for kind in [
            Marker::ExactlyOne,
            Marker::ZeroOrOne,
            Marker::OneOrMany,
            Marker::ZeroOrMany,
            Marker::Many,
        ] {
            let (paths, trim) = marker(kind, tip, Vec2::new(1.0, 0.0), &stroke, Color::WHITE);
            assert_eq!(paths.len(), if kind == Marker::Many { 1 } else { 2 });
            assert_eq!(trim, 0.0, "central prong reaches the attachment");
            assert!(paths.iter().all(|p| p.stroke.unwrap().dash.is_none()));
            if matches!(kind, Marker::Many | Marker::OneOrMany | Marker::ZeroOrMany) {
                let fork = &paths[0].path;
                assert_eq!(fork.elements().len(), 3);
                assert_eq!(
                    fork.elements()[1],
                    PathEl::LineTo(Point::new(tip.x - len, tip.y))
                );
                assert_eq!(fork.bounding_box().x1, tip.x);
            }
            let circles: Vec<_> = paths.iter().filter(|p| p.fill.is_some()).collect();
            assert_eq!(
                circles.len(),
                usize::from(matches!(kind, Marker::ZeroOrOne | Marker::ZeroOrMany))
            );
            assert!(circles.iter().all(|c| c.fill == Some(Color::WHITE)));
            // Rotating the route rotates every part of the marker together.
            let (rotated, _) = marker(kind, tip, Vec2::new(0.0, -1.0), &stroke, Color::WHITE);
            for (a, b) in paths.iter().zip(rotated) {
                let a = a.path.bounding_box();
                let b = b.path.bounding_box();
                assert!((a.width() - b.height()).abs() < 1e-9);
                assert!((a.height() - b.width()).abs() < 1e-9);
            }
        }
    }
}
