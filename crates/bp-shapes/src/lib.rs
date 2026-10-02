//! Shape libraries: every non-icon shape is a small declarative entry in a
//! TOML file, so adding a shape is a data change and users can later load
//! libraries of their own. See `libraries/basic.toml` for the format.

mod template;

pub use template::{Coord, PathTemplate};

use bp_geom::{Dir, ray_exit};
use bp_model::kurbo::{BezPath, Ellipse, PathEl, Point, Rect, RoundedRect, Shape};
use bp_model::{Dash, PortId, ShapeRef, Style, StyleValues};
use serde::Deserialize;
use std::collections::HashSet;
use std::fmt;
use std::sync::OnceLock;

/// Accuracy when converting curves to Béziers, in page units.
const CURVE_ACCURACY: f64 = 0.05;

const BUILTIN: [&str; 2] = [
    include_str!("../libraries/basic.toml"),
    include_str!("../libraries/flowchart.toml"),
];

#[derive(Clone, Debug, PartialEq)]
pub enum Outline {
    /// The box, with corners rounded by the style's corner radius.
    Rect,
    /// The box with fully rounded ends (a pill).
    Stadium,
    Ellipse,
    Path(PathTemplate),
}

/// A connection point declared by a shape definition, in box fractions.
#[derive(Clone, Debug, PartialEq)]
pub struct PortDef {
    pub id: PortId,
    pub x: f64,
    pub y: f64,
    pub dir: Dir,
}

/// A connection point on a placed shape.
#[derive(Clone, Debug, PartialEq)]
pub struct Port {
    pub id: PortId,
    pub at: Point,
    /// The direction a connector leaves the shape from this port.
    pub dir: Dir,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ShapeDef {
    pub reference: ShapeRef,
    pub name: String,
    pub keywords: Vec<String>,
    pub default_size: (f64, f64),
    pub outline: Outline,
    /// Paths drawn behind the outline with the same fill and stroke, for
    /// stacked shapes such as a multi-document.
    pub back: Vec<PathTemplate>,
    /// Extra lines stroked on top of the fill (the rim of a cylinder, the
    /// side bars of a predefined process).
    pub details: Option<PathTemplate>,
    /// Where text goes, as fractions of the box: `[x0, y0, x1, y1]`.
    pub text_area: [f64; 4],
    /// Declared ports; `None` means one port per side, where the outline
    /// meets the lines through the centre.
    pub ports: Option<Vec<PortDef>>,
    /// Style overrides that make this shape's defaults.
    pub style: Style,
}

impl ShapeDef {
    /// The fully resolved default style of this shape.
    pub fn default_style(&self) -> StyleValues {
        self.style.resolve(&StyleValues::default())
    }

    /// The outline drawn into `rect`. Closed outlines end with `ClosePath`.
    pub fn outline(&self, rect: Rect, corner_radius: f64) -> BezPath {
        let rect = rect.abs();
        let max_radius = rect.width().min(rect.height()) / 2.0;
        let mut path = match &self.outline {
            Outline::Rect if corner_radius > 0.0 => {
                RoundedRect::from_rect(rect, corner_radius.min(max_radius)).to_path(CURVE_ACCURACY)
            }
            Outline::Rect => rect.to_path(CURVE_ACCURACY),
            Outline::Stadium => RoundedRect::from_rect(rect, max_radius).to_path(CURVE_ACCURACY),
            Outline::Ellipse => Ellipse::from_rect(rect).to_path(CURVE_ACCURACY),
            Outline::Path(t) => return t.build(rect),
        };
        // kurbo's ellipse path ends without ClosePath; renderers need one
        // to fill the shape and join the stroke cleanly.
        if !matches!(path.elements().last(), Some(PathEl::ClosePath)) {
            path.close_path();
        }
        path
    }

    pub fn back(&self, rect: Rect) -> Vec<BezPath> {
        self.back.iter().map(|t| t.build(rect.abs())).collect()
    }

    pub fn details(&self, rect: Rect) -> Option<BezPath> {
        self.details.as_ref().map(|t| t.build(rect.abs()))
    }

    /// Whether the outline encloses an area that can be filled.
    pub fn is_closed(&self) -> bool {
        match &self.outline {
            Outline::Path(t) => t.is_closed(),
            _ => true,
        }
    }

    /// The text area placed in `rect`.
    pub fn text_box(&self, rect: Rect) -> Rect {
        let r = rect.abs();
        let [x0, y0, x1, y1] = self.text_area;
        Rect::new(
            r.x0 + x0 * r.width(),
            r.y0 + y0 * r.height(),
            r.x0 + x1 * r.width(),
            r.y0 + y1 * r.height(),
        )
    }

    /// The ports of the shape placed in `rect`.
    pub fn ports(&self, rect: Rect, corner_radius: f64) -> Vec<Port> {
        let r = rect.abs();
        if let Some(defs) = &self.ports {
            return defs
                .iter()
                .map(|p| Port {
                    id: p.id.clone(),
                    at: Point::new(r.x0 + p.x * r.width(), r.y0 + p.y * r.height()),
                    dir: p.dir,
                })
                .collect();
        }
        let center = r.center();
        let side = |dir: Dir| match dir {
            Dir::N => Point::new(center.x, r.y0),
            Dir::E => Point::new(r.x1, center.y),
            Dir::S => Point::new(center.x, r.y1),
            Dir::W => Point::new(r.x0, center.y),
        };
        let outline = match self.outline {
            // Side midpoints are exact for these, and cheaper.
            Outline::Rect | Outline::Stadium | Outline::Ellipse => None,
            Outline::Path(_) => {
                let mut all = self.outline(r, corner_radius);
                for back in self.back(r) {
                    all.extend(back);
                }
                Some(all)
            }
        };
        Dir::ALL
            .iter()
            .map(|&dir| {
                let at = outline
                    .as_ref()
                    .and_then(|o| ray_exit(o, center, dir.vec()))
                    .unwrap_or_else(|| side(dir));
                Port {
                    id: PortId::new(dir.name()),
                    at,
                    dir,
                }
            })
            .collect()
    }

    pub fn port(&self, rect: Rect, corner_radius: f64, id: &PortId) -> Option<Port> {
        self.ports(rect, corner_radius)
            .into_iter()
            .find(|p| &p.id == id)
    }
}

pub struct Library {
    pub id: String,
    pub name: String,
    pub shapes: Vec<ShapeDef>,
}

impl Library {
    /// Loads a library from TOML (see `libraries/basic.toml`).
    pub fn from_toml(src: &str) -> Result<Library, LibraryError> {
        let file: LibraryFile = toml::from_str(src).map_err(|e| LibraryError {
            library: String::new(),
            shape: None,
            message: e.to_string(),
        })?;
        let err = |shape: &str, message: String| LibraryError {
            library: file.id.clone(),
            shape: Some(shape.to_owned()),
            message,
        };
        if ShapeRef::parse(&format!("{}/x", file.id)).is_none() {
            return Err(LibraryError {
                library: file.id.clone(),
                shape: None,
                message: "library ids may use letters, digits, '-' and '_'".into(),
            });
        }
        let mut seen = HashSet::new();
        let mut shapes = Vec::new();
        for s in file.shapes {
            let reference = ShapeRef::parse(&format!("{}/{}", file.id, s.id)).ok_or_else(|| {
                err(
                    &s.id,
                    "shape ids may use letters, digits, '-' and '_'".into(),
                )
            })?;
            if !seen.insert(s.id.clone()) {
                return Err(err(&s.id, "duplicate shape id".into()));
            }
            let outline = match s.outline.as_str() {
                "rect" => Outline::Rect,
                "stadium" => Outline::Stadium,
                "ellipse" => Outline::Ellipse,
                data => Outline::Path(
                    PathTemplate::parse(data).map_err(|m| err(&s.id, format!("outline: {m}")))?,
                ),
            };
            let back = s
                .back
                .iter()
                .map(|d| PathTemplate::parse(d).map_err(|m| err(&s.id, format!("back: {m}"))))
                .collect::<Result<_, _>>()?;
            let details = s
                .details
                .as_deref()
                .map(PathTemplate::parse)
                .transpose()
                .map_err(|m| err(&s.id, format!("details: {m}")))?;
            let [w, h] = s.size;
            if !(w > 0.0 && h > 0.0) {
                return Err(err(&s.id, "size must be positive".into()));
            }
            let text_area = s.text_area.unwrap_or([0.0, 0.0, 1.0, 1.0]);
            let [x0, y0, x1, y1] = text_area;
            if !(0.0 <= x0 && x0 < x1 && x1 <= 1.0 && 0.0 <= y0 && y0 < y1 && y1 <= 1.0) {
                return Err(err(
                    &s.id,
                    "text_area must be [x0, y0, x1, y1] within 0..1".into(),
                ));
            }
            let ports = match s.ports {
                None => None,
                Some(list) => {
                    let mut ids = HashSet::new();
                    let mut out = Vec::new();
                    for p in list {
                        let dir = Dir::parse(&p.dir).ok_or_else(|| {
                            err(&s.id, format!("port {}: dir must be n, e, s or w", p.id))
                        })?;
                        if !ids.insert(p.id.clone()) {
                            return Err(err(&s.id, format!("duplicate port {}", p.id)));
                        }
                        out.push(PortDef {
                            id: PortId::new(&p.id),
                            x: p.x,
                            y: p.y,
                            dir,
                        });
                    }
                    Some(out)
                }
            };
            shapes.push(ShapeDef {
                reference,
                name: s.name,
                keywords: s.keywords,
                default_size: (w, h),
                outline,
                back,
                details,
                text_area,
                ports,
                style: s.style,
            });
        }
        Ok(Library {
            id: file.id,
            name: file.name,
            shapes,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LibraryError {
    pub library: String,
    pub shape: Option<String>,
    pub message: String,
}

impl fmt::Display for LibraryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.shape {
            Some(shape) => write!(f, "{}/{shape}: {}", self.library, self.message),
            None if self.library.is_empty() => f.write_str(&self.message),
            None => write!(f, "{}: {}", self.library, self.message),
        }
    }
}

impl std::error::Error for LibraryError {}

/// Every loaded shape library, in palette order.
pub struct Libraries {
    libraries: Vec<Library>,
    fallback: ShapeDef,
}

impl Libraries {
    pub fn new(libraries: Vec<Library>) -> Self {
        let fallback = ShapeDef {
            reference: ShapeRef::new("missing", "shape"),
            name: "Missing shape".into(),
            keywords: Vec::new(),
            default_size: (120.0, 60.0),
            outline: Outline::Rect,
            back: Vec::new(),
            details: None,
            text_area: [0.0, 0.0, 1.0, 1.0],
            ports: None,
            style: Style {
                dash: Some(Dash::Dashed),
                ..Style::default()
            },
        };
        Self {
            libraries,
            fallback,
        }
    }

    /// The libraries that ship with Blueprint.
    pub fn builtin() -> &'static Libraries {
        static BUILTIN_LIBRARIES: OnceLock<Libraries> = OnceLock::new();
        BUILTIN_LIBRARIES.get_or_init(|| {
            Libraries::new(
                BUILTIN
                    .iter()
                    .map(|src| Library::from_toml(src).expect("built-in libraries are valid"))
                    .collect(),
            )
        })
    }

    pub fn libraries(&self) -> &[Library] {
        &self.libraries
    }

    pub fn get(&self, reference: &ShapeRef) -> Option<&ShapeDef> {
        self.libraries
            .iter()
            .find(|l| l.id == reference.library())?
            .shapes
            .iter()
            .find(|s| s.reference.shape() == reference.shape())
    }

    /// The definition for `reference`, or a dashed box if it is unknown
    /// (for example, a file from a newer version), so nothing disappears.
    pub fn resolve(&self, reference: &ShapeRef) -> &ShapeDef {
        self.get(reference).unwrap_or(&self.fallback)
    }

    pub fn shapes(&self) -> impl Iterator<Item = &ShapeDef> {
        self.libraries.iter().flat_map(|l| &l.shapes)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LibraryFile {
    id: String,
    name: String,
    #[serde(rename = "shape", default)]
    shapes: Vec<ShapeFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ShapeFile {
    id: String,
    name: String,
    #[serde(default)]
    keywords: Vec<String>,
    size: [f64; 2],
    outline: String,
    #[serde(default)]
    back: Vec<String>,
    details: Option<String>,
    text_area: Option<[f64; 4]>,
    ports: Option<Vec<PortFile>>,
    #[serde(default)]
    style: Style,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PortFile {
    id: String,
    x: f64,
    y: f64,
    dir: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use bp_geom::distance_to_path;

    fn builtin() -> &'static Libraries {
        Libraries::builtin()
    }

    #[test]
    fn builtin_libraries_load() {
        let ids: Vec<_> = builtin()
            .libraries()
            .iter()
            .map(|l| l.id.as_str())
            .collect();
        assert_eq!(ids, ["basic", "flowchart"]);
        assert!(builtin().shapes().count() >= 40);
    }

    #[test]
    fn phase0_shapes_exist() {
        for name in [
            "rectangle",
            "rounded-rectangle",
            "ellipse",
            "diamond",
            "text",
        ] {
            assert!(
                builtin().get(&ShapeRef::new("basic", name)).is_some(),
                "{name}"
            );
        }
    }

    #[test]
    fn every_builtin_shape_is_sane() {
        let rect = Rect::new(100.0, 50.0, 260.0, 150.0);
        for def in builtin().shapes() {
            let style = def.default_style();
            let outline = def.outline(rect, style.corner_radius);
            let mut all = outline.clone();
            for b in def.back(rect) {
                all.extend(b);
            }
            let bbox = all.bounding_box();
            // Closed outlines (with any back sheets) fill their box, within
            // a hair for curves. Open ones, like a bracket, need not.
            assert!(
                !def.is_closed()
                    || (bbox.x0 - rect.x0).abs() < 1.0
                        && (bbox.y0 - rect.y0).abs() < 1.0
                        && (bbox.x1 - rect.x1).abs() < 1.0
                        && (bbox.y1 - rect.y1).abs() < 1.0,
                "{}: outline {bbox:?} does not fill {rect:?}",
                def.reference
            );
            let tb = def.text_box(rect);
            assert!(
                rect.contains(tb.origin()) && tb.area() > 0.0,
                "{}",
                def.reference
            );
            // Ports sit on the outline (or a back sheet) and are unique.
            let ports = def.ports(rect, style.corner_radius);
            assert!(!ports.is_empty(), "{}", def.reference);
            for p in &ports {
                let d = distance_to_path(&all, p.at);
                assert!(
                    d < 0.5,
                    "{}: port {} is {d} off the outline",
                    def.reference,
                    p.id.as_str()
                );
            }
            let ids: HashSet<_> = ports.iter().map(|p| p.id.clone()).collect();
            assert_eq!(ids.len(), ports.len(), "{}", def.reference);
            assert!(!def.name.is_empty());
        }
    }

    #[test]
    fn default_ports_follow_the_outline() {
        let data = builtin().get(&ShapeRef::new("flowchart", "data")).unwrap();
        let rect = Rect::new(0.0, 0.0, 100.0, 50.0);
        let west = data.port(rect, 0.0, &PortId::new("w")).unwrap();
        // The slanted left side of the parallelogram is at x = 10 halfway down.
        assert!((west.at.x - 10.0).abs() < 1e-6, "{west:?}");
        assert_eq!(west.dir, Dir::W);
    }

    #[test]
    fn unknown_shapes_resolve_to_a_dashed_box() {
        let def = builtin().resolve(&ShapeRef::new("future", "thing"));
        assert_eq!(def.outline, Outline::Rect);
        assert_eq!(def.default_style().dash, Dash::Dashed);
    }

    #[test]
    fn text_shapes_have_no_fill_or_stroke() {
        let text = builtin().get(&ShapeRef::new("basic", "text")).unwrap();
        let style = text.default_style();
        assert_eq!((style.fill, style.stroke), (None, None));
    }

    #[test]
    fn library_errors_name_the_shape() {
        let src = r#"
            id = "mine"
            name = "Mine"
            [[shape]]
            id = "blob"
            name = "Blob"
            size = [10, 10]
            outline = "L 0 0"
        "#;
        let err = Library::from_toml(src).err().unwrap();
        assert_eq!(err.shape.as_deref(), Some("blob"));
        assert!(err.to_string().starts_with("mine/blob: outline:"), "{err}");
        let dup = r#"
            id = "mine"
            name = "Mine"
            [[shape]]
            id = "a"
            name = "A"
            size = [10, 10]
            outline = "rect"
            [[shape]]
            id = "a"
            name = "A again"
            size = [10, 10]
            outline = "rect"
        "#;
        assert!(Library::from_toml(dup).is_err());
        let typo = "id = \"x\"\nname = \"X\"\n[[shape]]\nid = \"a\"\nname = \"A\"\nsize = [1, 1]\noutline = \"rect\"\ncolour = \"red\"\n";
        assert!(
            Library::from_toml(typo).is_err(),
            "unknown keys are rejected"
        );
    }
}
