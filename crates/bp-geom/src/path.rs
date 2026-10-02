//! Hit tests and measurements on paths and polylines.

use kurbo::{BezPath, Line, ParamCurve, ParamCurveNearest, Point, Shape, Vec2};

/// Accuracy of nearest-point searches, in page units.
const ACCURACY: f64 = 0.01;

/// Where a ray from `from` heading along `toward` last crosses `path`, or
/// `None` if it never does. "Last" matters for concave outlines: the ray
/// leaves the shape at its farthest crossing.
pub fn ray_exit(path: &BezPath, from: Point, toward: Vec2) -> Option<Point> {
    let len = toward.hypot();
    if len == 0.0 {
        return None;
    }
    let bbox = path.bounding_box();
    let reach = (bbox.width() + bbox.height()) * 2.0 + (from - bbox.center()).hypot() + 1.0;
    let line = Line::new(from, from + toward * (reach / len));
    path.segments()
        .flat_map(|seg| seg.intersect_line(line))
        .map(|hit| hit.line_t)
        .max_by(f64::total_cmp)
        .map(|t| line.eval(t))
}

/// The shortest distance from `p` to the outline of `path`.
pub fn distance_to_path(path: &BezPath, p: Point) -> f64 {
    path.segments()
        .map(|seg| seg.nearest(p, ACCURACY).distance_sq)
        .fold(f64::INFINITY, f64::min)
        .sqrt()
}

/// The shortest distance from `p` to the polyline through `points`.
pub fn distance_to_polyline(points: &[Point], p: Point) -> f64 {
    match points {
        [] => f64::INFINITY,
        [only] => (*only - p).hypot(),
        _ => points
            .windows(2)
            .map(|w| Line::new(w[0], w[1]).nearest(p, ACCURACY).distance_sq)
            .fold(f64::INFINITY, f64::min)
            .sqrt(),
    }
}

pub fn polyline_length(points: &[Point]) -> f64 {
    points.windows(2).map(|w| (w[1] - w[0]).hypot()).sum()
}

/// The point at fraction `t` (0–1) of the polyline's length, with the unit
/// direction of the segment it falls on.
pub fn polyline_point_at(points: &[Point], t: f64) -> Option<(Point, Vec2)> {
    let first = *points.first()?;
    let total = polyline_length(points);
    if total == 0.0 {
        return Some((first, Vec2::new(1.0, 0.0)));
    }
    let mut remaining = total * t.clamp(0.0, 1.0);
    for w in points.windows(2) {
        let seg = w[1] - w[0];
        let len = seg.hypot();
        if len == 0.0 {
            continue;
        }
        if remaining <= len {
            return Some((w[0] + seg * (remaining / len), seg / len));
        }
        remaining -= len;
    }
    let n = points.len();
    let seg = points[n - 1] - points[n - 2];
    Some((points[n - 1], seg / seg.hypot().max(f64::EPSILON)))
}

/// Drops repeated points and middle points of straight runs.
pub fn simplify_polyline(points: &mut Vec<Point>) {
    points.dedup_by(|b, a| (*b - *a).hypot() < 1e-6);
    let mut i = 1;
    while i + 1 < points.len() {
        let (a, b, c) = (points[i - 1], points[i], points[i + 1]);
        let cross = (b - a).cross(c - b);
        let forward = (b - a).dot(c - b) > 0.0;
        if cross.abs() < 1e-6 && forward {
            points.remove(i);
        } else {
            i += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::{Circle, Rect};

    #[test]
    fn ray_exit_finds_the_far_side() {
        let rect = Rect::new(0.0, 0.0, 100.0, 50.0).to_path(0.1);
        let hit = ray_exit(&rect, Point::new(50.0, 25.0), Vec2::new(1.0, 0.0)).unwrap();
        assert!((hit - Point::new(100.0, 25.0)).hypot() < 1e-9);
        let circle = Circle::new((0.0, 0.0), 10.0).to_path(0.01);
        let hit = ray_exit(&circle, Point::ZERO, Vec2::new(0.0, -2.0)).unwrap();
        assert!((hit - Point::new(0.0, -10.0)).hypot() < 1e-3);
        assert!(ray_exit(&rect, Point::new(500.0, 500.0), Vec2::new(1.0, 0.0)).is_none());
    }

    #[test]
    fn distances() {
        let rect = Rect::new(0.0, 0.0, 10.0, 10.0).to_path(0.1);
        assert!((distance_to_path(&rect, Point::new(5.0, 13.0)) - 3.0).abs() < 1e-9);
        let line = [
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(10.0, 10.0),
        ];
        assert!((distance_to_polyline(&line, Point::new(12.0, 5.0)) - 2.0).abs() < 1e-9);
        assert_eq!(polyline_length(&line), 20.0);
    }

    #[test]
    fn point_at_fraction() {
        let line = [
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(10.0, 10.0),
        ];
        let (p, dir) = polyline_point_at(&line, 0.75).unwrap();
        assert_eq!(p, Point::new(10.0, 5.0));
        assert_eq!(dir, Vec2::new(0.0, 1.0));
        assert_eq!(polyline_point_at(&line, 0.0).unwrap().0, Point::ZERO);
    }

    #[test]
    fn simplify_removes_redundant_points() {
        let mut pts = vec![
            Point::new(0.0, 0.0),
            Point::new(0.0, 0.0),
            Point::new(5.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(10.0, 10.0),
        ];
        simplify_polyline(&mut pts);
        assert_eq!(
            pts,
            vec![
                Point::new(0.0, 0.0),
                Point::new(10.0, 0.0),
                Point::new(10.0, 10.0)
            ]
        );
        // A U-turn is not a straight run.
        let mut back = vec![
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(5.0, 0.0),
        ];
        simplify_polyline(&mut back);
        assert_eq!(back.len(), 3);
    }
}
