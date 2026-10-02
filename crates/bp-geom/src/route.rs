//! Orthogonal connector routing.
//!
//! The router builds a sparse grid from the interesting coordinates (the
//! edges of every obstacle grown by a margin, the connector's stubs, and
//! the midlines between them), drops grid points inside obstacles, and
//! runs A* over (point, heading) states. Each bend costs extra, and
//! running along a line that is not a midline costs slightly more, so ties
//! go to routes centred between shapes.
//!
//! Routes depend only on their inputs, so they can be cached and are the
//! same on screen and in every export.

use crate::{Dir, simplify_polyline};
use kurbo::{Point, Rect};
use std::cmp::Ordering;
use std::collections::BinaryHeap;

/// One possible end of a route.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Terminal {
    /// Where the route touches its shape (a port), or the free end point.
    pub point: Point,
    /// The direction the route leaves the shape from `point`; `None` for a
    /// free end, which may leave in any direction.
    pub dir: Option<Dir>,
    /// The shape's box, which the route keeps clear of.
    pub obstacle: Option<Rect>,
}

impl Terminal {
    pub fn free(point: Point) -> Self {
        Self {
            point,
            dir: None,
            obstacle: None,
        }
    }
}

/// The cost of one bend, in page units of length.
const BEND: f64 = 40.0;
/// Extra cost per unit of length along a line that is not a midline.
const OFF_CENTRE: f64 = 0.02;
const EPS: f64 = 1e-6;

/// A route and which source and target candidates it uses.
#[derive(Clone, Debug, PartialEq)]
pub struct Route {
    pub points: Vec<Point>,
    pub source: usize,
    pub target: usize,
}

/// The cheapest orthogonal route from any of `sources` to any of
/// `targets`, keeping `margin` clear of the terminals' shapes and of
/// `obstacles`. Several candidates per end let a connector that floats on
/// a shape's outline pick the best side.
///
/// # Panics
/// If `sources` or `targets` is empty.
pub fn route_orthogonal(
    sources: &[Terminal],
    targets: &[Terminal],
    obstacles: &[Rect],
    margin: f64,
) -> Route {
    assert!(!sources.is_empty() && !targets.is_empty());
    let margin = fit_margin(sources, targets, margin);
    let mut boxes: Vec<Rect> = obstacles.to_vec();
    for t in sources.iter().chain(targets) {
        if let Some(r) = t.obstacle
            && !boxes.contains(&r)
        {
            boxes.push(r);
        }
    }
    let grown: Vec<Rect> = boxes.iter().map(|b| b.inflate(margin, margin)).collect();
    let source_stubs: Vec<Point> = sources.iter().map(|t| stub(t, margin)).collect();
    let target_stubs: Vec<Point> = targets.iter().map(|t| stub(t, margin)).collect();

    let grid = Grid::new(&grown, source_stubs.iter().chain(&target_stubs));
    search(
        &grid,
        &grown,
        sources,
        targets,
        &source_stubs,
        &target_stubs,
    )
    .unwrap_or_else(|| fallback(sources, targets, &source_stubs, &target_stubs))
}

/// Shrinks the margin when the two shapes are close, so their grown boxes
/// don't swallow each other's stubs.
fn fit_margin(sources: &[Terminal], targets: &[Terminal], margin: f64) -> f64 {
    let a = sources.iter().find_map(|t| t.obstacle);
    let b = targets.iter().find_map(|t| t.obstacle);
    let (Some(a), Some(b)) = (a, b) else {
        return margin;
    };
    if a == b {
        return margin;
    }
    let gap = (b.x0 - a.x1)
        .max(a.x0 - b.x1)
        .max(b.y0 - a.y1)
        .max(a.y0 - b.y1);
    if gap > 0.0 {
        margin.min(gap / 2.0).max(2.0)
    } else {
        margin.min(4.0)
    }
}

/// Where the route turns onto the grid: out from the port along its
/// direction, at least `margin` and at least clear of its own shape.
fn stub(t: &Terminal, margin: f64) -> Point {
    let Some(dir) = t.dir else {
        return t.point;
    };
    let mut p = t.point + dir.vec() * margin;
    if let Some(r) = t.obstacle.map(|r| r.inflate(margin, margin)) {
        match dir {
            Dir::E => p.x = p.x.max(r.x1),
            Dir::W => p.x = p.x.min(r.x0),
            Dir::S => p.y = p.y.max(r.y1),
            Dir::N => p.y = p.y.min(r.y0),
        }
    }
    p
}

fn strictly_inside(r: &Rect, p: Point) -> bool {
    p.x > r.x0 + EPS && p.x < r.x1 - EPS && p.y > r.y0 + EPS && p.y < r.y1 - EPS
}

struct Axis {
    values: Vec<f64>,
    /// Whether each value is a midline between two other coordinates.
    mid: Vec<bool>,
}

impl Axis {
    fn new(mut base: Vec<f64>) -> Self {
        base.sort_by(f64::total_cmp);
        base.dedup_by(|b, a| (*b - *a).abs() < EPS);
        let mut values = Vec::with_capacity(base.len() * 2);
        let mut mid = Vec::with_capacity(base.len() * 2);
        for (i, &v) in base.iter().enumerate() {
            if i > 0 {
                values.push((base[i - 1] + v) / 2.0);
                mid.push(true);
            }
            values.push(v);
            mid.push(false);
        }
        Self { values, mid }
    }

    fn index_of(&self, v: f64) -> usize {
        self.values
            .iter()
            .position(|x| (x - v).abs() < EPS)
            .expect("every stub coordinate is on the grid")
    }
}

struct Grid {
    xs: Axis,
    ys: Axis,
    /// For each grid point, whether it is usable.
    open: Vec<bool>,
}

impl Grid {
    fn new<'a>(grown: &[Rect], stubs: impl Iterator<Item = &'a Point>) -> Self {
        let stubs: Vec<Point> = stubs.copied().collect();
        let mut xs: Vec<f64> = stubs.iter().map(|p| p.x).collect();
        let mut ys: Vec<f64> = stubs.iter().map(|p| p.y).collect();
        for r in grown {
            xs.extend([r.x0, r.x1]);
            ys.extend([r.y0, r.y1]);
        }
        let (xs, ys) = (Axis::new(xs), Axis::new(ys));
        let mut open = vec![false; xs.values.len() * ys.values.len()];
        for (j, &y) in ys.values.iter().enumerate() {
            for (i, &x) in xs.values.iter().enumerate() {
                let p = Point::new(x, y);
                open[j * xs.values.len() + i] = !grown.iter().any(|r| strictly_inside(r, p));
            }
        }
        let mut grid = Self { xs, ys, open };
        // Stubs are always usable, even when shapes overlap.
        for p in stubs {
            let n = grid.node_at(p);
            grid.open[n] = true;
        }
        grid
    }

    fn width(&self) -> usize {
        self.xs.values.len()
    }

    fn len(&self) -> usize {
        self.open.len()
    }

    fn node_at(&self, p: Point) -> usize {
        self.ys.index_of(p.y) * self.width() + self.xs.index_of(p.x)
    }

    fn point(&self, node: usize) -> Point {
        let (i, j) = (node % self.width(), node / self.width());
        Point::new(self.xs.values[i], self.ys.values[j])
    }

    /// The next grid point from `node` heading `dir`, if usable.
    fn step(&self, node: usize, dir: Dir) -> Option<usize> {
        let (w, h) = (self.width(), self.ys.values.len());
        let (i, j) = (node % w, node / w);
        let (ni, nj) = match dir {
            Dir::E if i + 1 < w => (i + 1, j),
            Dir::W if i > 0 => (i - 1, j),
            Dir::S if j + 1 < h => (i, j + 1),
            Dir::N if j > 0 => (i, j - 1),
            _ => return None,
        };
        let next = nj * w + ni;
        self.open[next].then_some(next)
    }

    /// Whether the line a step from `node` heading `dir` runs along is a
    /// midline.
    fn on_midline(&self, node: usize, dir: Dir) -> bool {
        let (i, j) = (node % self.width(), node / self.width());
        if dir.is_horizontal() {
            self.ys.mid[j]
        } else {
            self.xs.mid[i]
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
struct Entry {
    /// Cost so far plus the estimate of what remains (A*).
    priority: f64,
    /// Cost so far.
    cost: f64,
    state: usize,
}

impl Eq for Entry {}

impl Ord for Entry {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reversed: BinaryHeap is a max-heap and we want the cheapest.
        other
            .priority
            .total_cmp(&self.priority)
            .then(other.state.cmp(&self.state))
    }
}

impl PartialOrd for Entry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Heading index for search states; 4 means "not moving yet".
fn heading(dir: Option<Dir>) -> usize {
    match dir {
        Some(Dir::N) => 0,
        Some(Dir::E) => 1,
        Some(Dir::S) => 2,
        Some(Dir::W) => 3,
        None => 4,
    }
}

fn dir_of(heading: usize) -> Option<Dir> {
    [Some(Dir::N), Some(Dir::E), Some(Dir::S), Some(Dir::W), None][heading]
}

#[derive(Clone, Copy)]
enum Prev {
    Unvisited,
    Start(usize),
    From(usize),
}

fn search(
    grid: &Grid,
    grown: &[Rect],
    sources: &[Terminal],
    targets: &[Terminal],
    source_stubs: &[Point],
    target_stubs: &[Point],
) -> Option<Route> {
    let states = grid.len() * 5;
    let goal = states;
    let mut best = vec![f64::INFINITY; states + 1];
    let mut prev = vec![Prev::Unvisited; states + 1];
    let mut goal_target = 0;
    let mut heap = BinaryHeap::new();

    // The A* estimate: the Manhattan distance to the nearest target stub
    // plus that stub's length. It never overestimates (every step costs
    // at least its length and bends only add), so routes stay optimal.
    let ends: Vec<(Point, f64)> = targets
        .iter()
        .zip(target_stubs)
        .map(|(t, stub)| (*stub, (*stub - t.point).hypot()))
        .collect();
    let estimate = |node: usize| {
        let p = grid.point(node);
        ends.iter()
            .map(|(stub, len)| (p.x - stub.x).abs() + (p.y - stub.y).abs() + len)
            .fold(f64::INFINITY, f64::min)
    };

    for (k, (t, stub)) in sources.iter().zip(source_stubs).enumerate() {
        let node = grid.node_at(*stub);
        let state = node * 5 + heading(t.dir);
        let cost = (*stub - t.point).hypot();
        if cost < best[state] {
            best[state] = cost;
            prev[state] = Prev::Start(k);
            heap.push(Entry {
                priority: cost + estimate(node),
                cost,
                state,
            });
        }
    }
    let target_nodes: Vec<usize> = target_stubs.iter().map(|p| grid.node_at(*p)).collect();

    while let Some(Entry { cost, state, .. }) = heap.pop() {
        if state == goal {
            break;
        }
        if cost > best[state] {
            continue;
        }
        let (node, head) = (state / 5, state % 5);
        let moving = dir_of(head);

        for (k, &tn) in target_nodes.iter().enumerate() {
            if tn != node {
                continue;
            }
            let t = &targets[k];
            // Entering the port means heading opposite to its direction.
            let entry = match (t.dir, moving) {
                (None, _) | (_, None) => 0.0,
                (Some(d), Some(m)) if m == d.opposite() => 0.0,
                (Some(d), Some(m)) if m == d => continue,
                _ => BEND,
            };
            let total = cost + entry + (target_stubs[k] - t.point).hypot();
            if total < best[goal] {
                best[goal] = total;
                prev[goal] = Prev::From(state);
                goal_target = k;
                heap.push(Entry {
                    priority: total,
                    cost: total,
                    state: goal,
                });
            }
        }

        for dir in Dir::ALL {
            if moving == Some(dir.opposite()) {
                continue;
            }
            let Some(next) = grid.step(node, dir) else {
                continue;
            };
            let (a, b) = (grid.point(node), grid.point(next));
            let middle = a.midpoint(b);
            if grown.iter().any(|r| strictly_inside(r, middle)) {
                continue;
            }
            let len = (b - a).hypot();
            let weight = if grid.on_midline(node, dir) {
                1.0
            } else {
                1.0 + OFF_CENTRE
            };
            let bend = if moving.is_some_and(|m| m != dir) {
                BEND
            } else {
                0.0
            };
            let next_state = next * 5 + heading(Some(dir));
            let next_cost = cost + len * weight + bend;
            if next_cost < best[next_state] {
                best[next_state] = next_cost;
                prev[next_state] = Prev::From(state);
                heap.push(Entry {
                    priority: next_cost + estimate(next),
                    cost: next_cost,
                    state: next_state,
                });
            }
        }
    }

    if best[goal].is_infinite() {
        return None;
    }
    let mut nodes = Vec::new();
    let mut at = goal;
    let source = loop {
        match prev[at] {
            Prev::From(p) => {
                nodes.push(p / 5);
                at = p;
            }
            Prev::Start(k) => break k,
            Prev::Unvisited => return None,
        }
    };
    nodes.reverse();
    let mut points = vec![sources[source].point];
    points.extend(nodes.into_iter().map(|n| grid.point(n)));
    points.push(targets[goal_target].point);
    simplify_polyline(&mut points);
    Some(Route {
        points,
        source,
        target: goal_target,
    })
}

/// A plain elbow route that ignores obstacles, for when no clear route
/// exists (for example, overlapping shapes).
fn fallback(
    sources: &[Terminal],
    targets: &[Terminal],
    source_stubs: &[Point],
    target_stubs: &[Point],
) -> Route {
    let (s, t) = (source_stubs[0], target_stubs[0]);
    let mid = s.midpoint(t);
    let horizontal_first = sources[0].dir.is_none_or(Dir::is_horizontal);
    let elbow = if horizontal_first {
        [Point::new(mid.x, s.y), Point::new(mid.x, t.y)]
    } else {
        [Point::new(s.x, mid.y), Point::new(t.x, mid.y)]
    };
    let mut points = vec![sources[0].point, s, elbow[0], elbow[1], t, targets[0].point];
    simplify_polyline(&mut points);
    Route {
        points,
        source: 0,
        target: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn port(rect: Rect, dir: Dir) -> Terminal {
        let c = rect.center();
        let point = match dir {
            Dir::N => Point::new(c.x, rect.y0),
            Dir::E => Point::new(rect.x1, c.y),
            Dir::S => Point::new(c.x, rect.y1),
            Dir::W => Point::new(rect.x0, c.y),
        };
        Terminal {
            point,
            dir: Some(dir),
            obstacle: Some(rect),
        }
    }

    fn sides(rect: Rect) -> Vec<Terminal> {
        Dir::ALL.iter().map(|d| port(rect, *d)).collect()
    }

    fn is_orthogonal(points: &[Point]) -> bool {
        points
            .windows(2)
            .all(|w| (w[0].x - w[1].x).abs() < 1e-9 || (w[0].y - w[1].y).abs() < 1e-9)
    }

    fn crosses(points: &[Point], r: Rect) -> bool {
        points.windows(2).any(|w| {
            // Sample along the segment: enough for axis-aligned segments.
            (1..20).any(|i| {
                let p = w[0].lerp(w[1], f64::from(i) / 20.0);
                strictly_inside(&r, p)
            })
        })
    }

    #[test]
    fn aligned_ports_give_a_straight_line() {
        let a = Rect::new(0.0, 0.0, 100.0, 50.0);
        let b = Rect::new(200.0, 0.0, 300.0, 50.0);
        let r = route_orthogonal(&[port(a, Dir::E)], &[port(b, Dir::W)], &[], 20.0);
        assert_eq!(
            r.points,
            vec![Point::new(100.0, 25.0), Point::new(200.0, 25.0)]
        );
    }

    #[test]
    fn offset_ports_bend_on_the_midline() {
        let a = Rect::new(0.0, 0.0, 100.0, 50.0);
        let b = Rect::new(200.0, 100.0, 300.0, 150.0);
        let r = route_orthogonal(&[port(a, Dir::E)], &[port(b, Dir::W)], &[], 20.0);
        assert_eq!(
            r.points,
            vec![
                Point::new(100.0, 25.0),
                Point::new(150.0, 25.0),
                Point::new(150.0, 125.0),
                Point::new(200.0, 125.0),
            ]
        );
    }

    #[test]
    fn routes_around_its_own_shapes() {
        // Leaving A eastwards towards B, which is directly behind A.
        let a = Rect::new(200.0, 0.0, 300.0, 50.0);
        let b = Rect::new(0.0, 0.0, 100.0, 50.0);
        let r = route_orthogonal(&[port(a, Dir::E)], &[port(b, Dir::W)], &[], 20.0);
        assert!(is_orthogonal(&r.points));
        assert!(
            !crosses(&r.points, a) && !crosses(&r.points, b),
            "{:?}",
            r.points
        );
        assert_eq!(r.points[0], Point::new(300.0, 25.0));
        assert_eq!(*r.points.last().unwrap(), Point::new(0.0, 25.0));
    }

    #[test]
    fn floating_ends_pick_facing_sides() {
        let a = Rect::new(0.0, 0.0, 100.0, 50.0);
        let b = Rect::new(0.0, 200.0, 100.0, 250.0);
        let r = route_orthogonal(&sides(a), &sides(b), &[], 20.0);
        assert_eq!(
            r.points,
            vec![Point::new(50.0, 50.0), Point::new(50.0, 200.0)]
        );
        assert_eq!(
            (r.source, r.target),
            (2, 0),
            "a's south side to b's north side"
        );
    }

    #[test]
    fn diagonal_floating_ends_use_one_bend() {
        let a = Rect::new(0.0, 0.0, 100.0, 50.0);
        let b = Rect::new(300.0, 300.0, 400.0, 350.0);
        let r = route_orthogonal(&sides(a), &sides(b), &[], 20.0);
        assert!(is_orthogonal(&r.points));
        assert_eq!(r.points.len(), 3, "an L: {:?}", r.points);
    }

    #[test]
    fn avoids_extra_obstacles() {
        let a = Rect::new(0.0, 0.0, 100.0, 50.0);
        let b = Rect::new(400.0, 0.0, 500.0, 50.0);
        let wall = Rect::new(200.0, -100.0, 250.0, 150.0);
        let r = route_orthogonal(&[port(a, Dir::E)], &[port(b, Dir::W)], &[wall], 20.0);
        assert!(is_orthogonal(&r.points));
        assert!(!crosses(&r.points, wall), "{:?}", r.points);
    }

    #[test]
    fn free_ends_connect_directly() {
        let r = route_orthogonal(
            &[Terminal::free(Point::new(0.0, 0.0))],
            &[Terminal::free(Point::new(100.0, 50.0))],
            &[],
            20.0,
        );
        assert!(is_orthogonal(&r.points));
        assert_eq!(r.points.len(), 3);
    }

    #[test]
    fn overlapping_shapes_still_get_a_route() {
        let a = Rect::new(0.0, 0.0, 100.0, 100.0);
        let b = Rect::new(50.0, 50.0, 150.0, 150.0);
        let r = route_orthogonal(&[port(a, Dir::E)], &[port(b, Dir::S)], &[], 20.0);
        assert!(is_orthogonal(&r.points));
        assert_eq!(r.points[0], Point::new(100.0, 50.0));
        assert_eq!(*r.points.last().unwrap(), Point::new(100.0, 150.0));
    }

    #[test]
    fn close_shapes_shrink_the_margin() {
        let a = Rect::new(0.0, 0.0, 100.0, 50.0);
        let b = Rect::new(110.0, 60.0, 210.0, 110.0);
        let r = route_orthogonal(&[port(a, Dir::E)], &[port(b, Dir::W)], &[], 20.0);
        assert!(is_orthogonal(&r.points));
        assert!(
            !crosses(&r.points, a) && !crosses(&r.points, b),
            "{:?}",
            r.points
        );
        assert_eq!(
            r.points.len(),
            4,
            "a tight Z between the shapes: {:?}",
            r.points
        );
    }
}
