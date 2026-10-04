//! Outlines written as SVG-like path data in box-relative coordinates.
//!
//! A coordinate is a fraction of the shape's box (`0` is the left or top
//! edge, `1` the right or bottom edge), optionally followed by an offset in
//! page units: `1-12` is 12 units in from the right edge, `0+8` is 8 units
//! in from the left. Offsets keep details such as folded corners the same
//! size however large the shape is drawn.
//!
//! Commands are absolute only: `M x y`, `L x y`, `H x`, `V y`,
//! `C x1 y1 x2 y2 x y`, `Q x1 y1 x y`, `A rx ry rotation large sweep x y`
//! and `Z`. Arguments may repeat (`L 1 0 1 1`), as in SVG.

use bp_model::kurbo::{Arc, BezPath, Point, Rect, SvgArc, Vec2};

/// Accuracy when converting arcs to curves, in page units.
const ARC_ACCURACY: f64 = 0.05;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Coord {
    pub frac: f64,
    pub abs: f64,
}

impl Coord {
    /// The position along an axis that starts at `origin` and is `size`
    /// long.
    pub fn at(self, origin: f64, size: f64) -> f64 {
        origin + self.frac * size + self.abs
    }

    fn parse(token: &str) -> Result<Coord, String> {
        let bad = || format!("bad coordinate {token:?}");
        // Split at a sign that is not the first character: "1-12".
        let split = token
            .char_indices()
            .skip(1)
            .find(|(_, c)| *c == '+' || *c == '-')
            .map(|(i, _)| i);
        let (frac, abs) = match split {
            Some(i) => (&token[..i], &token[i..]),
            None => (token, "0"),
        };
        let frac: f64 = frac.parse().map_err(|_| bad())?;
        let abs: f64 = abs.parse().map_err(|_| bad())?;
        if frac.is_finite() && abs.is_finite() {
            Ok(Coord { frac, abs })
        } else {
            Err(bad())
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Cmd {
    Move(Coord, Coord),
    Line(Coord, Coord),
    Horizontal(Coord),
    Vertical(Coord),
    Cubic([Coord; 6]),
    Quad([Coord; 4]),
    Arc {
        rx: Coord,
        ry: Coord,
        rotation: f64,
        large: bool,
        sweep: bool,
        x: Coord,
        y: Coord,
    },
    Close,
}

/// A parsed outline that can be drawn into any box.
#[derive(Clone, Debug, PartialEq)]
pub struct PathTemplate {
    cmds: Vec<Cmd>,
}

impl PathTemplate {
    pub fn parse(src: &str) -> Result<Self, String> {
        let tokens = tokenize(src)?;
        let mut cmds = Vec::new();
        let mut i = 0;
        let mut command: Option<char> = None;
        while i < tokens.len() {
            let letter = match tokens[i] {
                Token::Command(c) => {
                    i += 1;
                    c
                }
                // Repeated arguments continue the previous command (a
                // repeated move continues as lines, as in SVG).
                Token::Number(_) => match command {
                    Some('M') => 'L',
                    Some('Z') | None => return Err("path data must start with a command".into()),
                    Some(c) => c,
                },
            };
            let arity = match letter {
                'M' | 'L' => 2,
                'H' | 'V' => 1,
                'C' => 6,
                'Q' => 4,
                'A' => 7,
                'Z' => 0,
                other => {
                    return Err(format!(
                        "unknown command {other:?} (only absolute commands MLHVCQAZ)"
                    ));
                }
            };
            if cmds.is_empty() && letter != 'M' {
                return Err("path data must start with M".into());
            }
            let args: Vec<Coord> = (0..arity)
                .map(|k| match tokens.get(i + k) {
                    Some(Token::Number(n)) => Coord::parse(n),
                    _ => Err(format!("{letter} needs {arity} numbers")),
                })
                .collect::<Result<_, _>>()?;
            i += arity;
            cmds.push(match letter {
                'M' => Cmd::Move(args[0], args[1]),
                'L' => Cmd::Line(args[0], args[1]),
                'H' => Cmd::Horizontal(args[0]),
                'V' => Cmd::Vertical(args[0]),
                'C' => Cmd::Cubic([args[0], args[1], args[2], args[3], args[4], args[5]]),
                'Q' => Cmd::Quad([args[0], args[1], args[2], args[3]]),
                'A' => Cmd::Arc {
                    rx: args[0],
                    ry: args[1],
                    rotation: args[2].frac,
                    large: args[3].frac != 0.0,
                    sweep: args[4].frac != 0.0,
                    x: args[5],
                    y: args[6],
                },
                _ => Cmd::Close,
            });
            command = Some(letter);
        }
        if cmds.is_empty() {
            return Err("empty path data".into());
        }
        Ok(Self { cmds })
    }

    /// Whether the outline encloses an area (its last command is `Z`).
    pub fn is_closed(&self) -> bool {
        matches!(self.cmds.last(), Some(Cmd::Close))
    }

    /// The outline drawn into `rect`.
    pub fn build(&self, rect: Rect) -> BezPath {
        let (w, h) = (rect.width(), rect.height());
        let pt = |x: Coord, y: Coord| Point::new(x.at(rect.x0, w), y.at(rect.y0, h));
        let mut path = BezPath::new();
        let mut current = Point::new(rect.x0, rect.y0);
        let mut start = current;
        for cmd in &self.cmds {
            match *cmd {
                Cmd::Move(x, y) => {
                    current = pt(x, y);
                    start = current;
                    path.move_to(current);
                }
                Cmd::Line(x, y) => {
                    current = pt(x, y);
                    path.line_to(current);
                }
                Cmd::Horizontal(x) => {
                    current = Point::new(x.at(rect.x0, w), current.y);
                    path.line_to(current);
                }
                Cmd::Vertical(y) => {
                    current = Point::new(current.x, y.at(rect.y0, h));
                    path.line_to(current);
                }
                Cmd::Cubic([x1, y1, x2, y2, x, y]) => {
                    current = pt(x, y);
                    path.curve_to(pt(x1, y1), pt(x2, y2), current);
                }
                Cmd::Quad([x1, y1, x, y]) => {
                    current = pt(x, y);
                    path.quad_to(pt(x1, y1), current);
                }
                Cmd::Arc {
                    rx,
                    ry,
                    rotation,
                    large,
                    sweep,
                    x,
                    y,
                } => {
                    let to = pt(x, y);
                    let arc = SvgArc {
                        from: current,
                        to,
                        radii: Vec2::new(rx.at(0.0, w).abs(), ry.at(0.0, h).abs()),
                        x_rotation: rotation.to_radians(),
                        large_arc: large,
                        sweep,
                    };
                    match Arc::from_svg_arc(&arc) {
                        Some(arc) => {
                            for el in arc.append_iter(ARC_ACCURACY) {
                                path.push(el);
                            }
                        }
                        None => path.line_to(to),
                    }
                    current = to;
                }
                Cmd::Close => {
                    path.close_path();
                    current = start;
                }
            }
        }
        path
    }
}

#[derive(Debug, PartialEq)]
enum Token<'a> {
    Command(char),
    Number(&'a str),
}

fn tokenize(src: &str) -> Result<Vec<Token<'_>>, String> {
    let mut tokens = Vec::new();
    let mut chars = src.char_indices().peekable();
    while let Some(&(i, c)) = chars.peek() {
        if c.is_whitespace() || c == ',' {
            chars.next();
        } else if c.is_ascii_alphabetic() {
            tokens.push(Token::Command(c.to_ascii_uppercase()));
            if c.is_ascii_lowercase() && c != 'z' {
                return Err(format!(
                    "relative command {c:?} is not supported; use {}",
                    c.to_ascii_uppercase()
                ));
            }
            chars.next();
        } else if c.is_ascii_digit() || matches!(c, '.' | '-' | '+') {
            let mut end = i;
            while let Some(&(j, d)) = chars.peek() {
                if d.is_ascii_digit() || matches!(d, '.' | '-' | '+') {
                    end = j + d.len_utf8();
                    chars.next();
                } else {
                    break;
                }
            }
            tokens.push(Token::Number(&src[i..end]));
        } else {
            return Err(format!("unexpected character {c:?}"));
        }
    }
    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bp_model::kurbo::{PathEl, Shape};

    #[test]
    fn coordinates_with_offsets() {
        assert_eq!(
            Coord::parse("0.5").unwrap(),
            Coord {
                frac: 0.5,
                abs: 0.0
            }
        );
        assert_eq!(
            Coord::parse("1-12").unwrap(),
            Coord {
                frac: 1.0,
                abs: -12.0
            }
        );
        assert_eq!(
            Coord::parse("0+8.5").unwrap(),
            Coord {
                frac: 0.0,
                abs: 8.5
            }
        );
        assert_eq!(
            Coord::parse("-0.1").unwrap(),
            Coord {
                frac: -0.1,
                abs: 0.0
            }
        );
        assert!(Coord::parse("1--").is_err());
        assert_eq!(Coord::parse("1-12").unwrap().at(10.0, 100.0), 98.0);
    }

    #[test]
    fn builds_into_any_box() {
        let t = PathTemplate::parse("M 0.5 0 L 1 0.5 0.5 1 0 0.5 Z").unwrap();
        assert!(t.is_closed());
        let path = t.build(Rect::new(100.0, 100.0, 300.0, 200.0));
        assert_eq!(path.elements()[0], PathEl::MoveTo(Point::new(200.0, 100.0)));
        assert_eq!(path.elements()[2], PathEl::LineTo(Point::new(200.0, 200.0)));
        assert_eq!(path.bounding_box(), Rect::new(100.0, 100.0, 300.0, 200.0));
    }

    #[test]
    fn offsets_stay_fixed_in_size() {
        let t = PathTemplate::parse("M 1-10 0 V 1").unwrap();
        assert!(!t.is_closed());
        for width in [50.0, 500.0] {
            let path = t.build(Rect::new(0.0, 0.0, width, 20.0));
            assert_eq!(
                path.elements()[0],
                PathEl::MoveTo(Point::new(width - 10.0, 0.0))
            );
            assert_eq!(
                path.elements()[1],
                PathEl::LineTo(Point::new(width - 10.0, 20.0))
            );
        }
    }

    #[test]
    fn arcs_scale_into_ellipses() {
        // A half ellipse bulging right, from top to bottom of the box.
        let t = PathTemplate::parse("M 0 0 A 1 0.5 0 0 1 0 1 Z").unwrap();
        let path = t.build(Rect::new(0.0, 0.0, 40.0, 100.0));
        let b = path.bounding_box();
        assert!((b.x1 - 40.0).abs() < 0.1, "{b:?}");
        assert!((b.y1 - 100.0).abs() < 1e-6);
    }

    #[test]
    fn rejects_bad_paths() {
        for bad in ["", "L 0 0", "M 0", "M 0 0 X 1 1", "m 0 0", "M 0 0 L 1 two"] {
            assert!(PathTemplate::parse(bad).is_err(), "{bad:?}");
        }
    }
}
