//! Pure geometry shared by the shape libraries, the scene builder and the
//! editor: directions, ray and hit tests on paths, polylines, the spatial
//! index, snapping, and orthogonal connector routing.

mod index;
mod path;
mod route;
mod snap;

pub use index::SpatialIndex;
pub use path::{
    distance_to_path, distance_to_polyline, polyline_length, polyline_point_at, ray_exit,
    simplify_polyline,
};
pub use route::{ROUTE_LANE_GAP, Route, Terminal, route_orthogonal, route_orthogonal_with_routes};
pub use snap::{Axis, Features, Guide, SnapResult, snap_point, snap_rect};

use kurbo::Vec2;

/// A compass direction on the page (y grows downwards).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Dir {
    N,
    E,
    S,
    W,
}

impl Dir {
    pub const ALL: [Dir; 4] = [Dir::N, Dir::E, Dir::S, Dir::W];

    /// Unit vector in page coordinates.
    pub fn vec(self) -> Vec2 {
        match self {
            Dir::N => Vec2::new(0.0, -1.0),
            Dir::E => Vec2::new(1.0, 0.0),
            Dir::S => Vec2::new(0.0, 1.0),
            Dir::W => Vec2::new(-1.0, 0.0),
        }
    }

    pub fn opposite(self) -> Dir {
        match self {
            Dir::N => Dir::S,
            Dir::E => Dir::W,
            Dir::S => Dir::N,
            Dir::W => Dir::E,
        }
    }

    pub fn is_horizontal(self) -> bool {
        matches!(self, Dir::E | Dir::W)
    }

    /// The direction closest to `v`, or `None` for a zero vector.
    pub fn from_vec(v: Vec2) -> Option<Dir> {
        if v.x == 0.0 && v.y == 0.0 {
            return None;
        }
        Some(if v.x.abs() >= v.y.abs() {
            if v.x > 0.0 { Dir::E } else { Dir::W }
        } else if v.y > 0.0 {
            Dir::S
        } else {
            Dir::N
        })
    }

    /// `n`, `e`, `s` or `w`: the names of the default ports.
    pub fn name(self) -> &'static str {
        match self {
            Dir::N => "n",
            Dir::E => "e",
            Dir::S => "s",
            Dir::W => "w",
        }
    }

    pub fn parse(s: &str) -> Option<Dir> {
        match s {
            "n" => Some(Dir::N),
            "e" => Some(Dir::E),
            "s" => Some(Dir::S),
            "w" => Some(Dir::W),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directions() {
        for d in Dir::ALL {
            assert_eq!(d.opposite().opposite(), d);
            assert_eq!(Dir::from_vec(d.vec()), Some(d));
            assert_eq!(Dir::parse(d.name()), Some(d));
        }
        assert_eq!(Dir::from_vec(Vec2::new(3.0, -1.0)), Some(Dir::E));
        assert_eq!(Dir::from_vec(Vec2::ZERO), None);
    }
}
