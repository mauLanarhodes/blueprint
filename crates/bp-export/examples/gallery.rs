//! Writes an SVG showing every built-in shape and the connector styles,
//! for eyeballing changes: `cargo run -p bp-export --example gallery -- out.svg`.

use bp_model::kurbo::{Point, Rect};
use bp_model::{
    Document, Element, ElementId, ElementKind, Endpoint, Marker, Parent, Routing, ShapeRef,
};
use bp_shapes::Libraries;

fn shape(
    doc: &mut Document,
    layer: Parent,
    shape: ShapeRef,
    bounds: Rect,
    text: &str,
) -> ElementId {
    let mut el = Element::shape(shape, layer, doc.next_order_key(layer), bounds);
    if let ElementKind::Shape(s) = &mut el.kind {
        s.text = text.into();
    }
    let id = el.id;
    doc.elements.insert(id, el);
    id
}

fn connector(
    doc: &mut Document,
    layer: Parent,
    source: Endpoint,
    target: Endpoint,
    edit: impl FnOnce(&mut bp_model::Connector),
) {
    let mut el = Element::connector(source, target, layer, doc.next_order_key(layer));
    if let ElementKind::Connector(c) = &mut el.kind {
        edit(c);
    }
    doc.elements.insert(el.id, el);
}

fn main() {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "gallery.svg".into());
    let mut doc = Document::new();
    let page = doc.first_page().unwrap();
    let layer = Parent::Layer(doc.layers_of(page)[0].id);
    let (cell_w, cell_h, cols) = (200.0, 150.0, 7);
    let mut i = 0;
    for lib in Libraries::builtin().libraries() {
        for def in &lib.shapes {
            let (col, row) = ((i % cols) as f64, (i / cols) as f64);
            let center = Point::new(col * cell_w + cell_w / 2.0, row * cell_h + cell_h / 2.0);
            let bounds = Rect::from_center_size(center, def.default_size);
            shape(&mut doc, layer, def.reference.clone(), bounds, &def.name);
            i += 1;
        }
    }
    // Each routing mode between two boxes, with a different start marker.
    let top = ((i / cols) + 1) as f64 * cell_h + 40.0;
    let process = ShapeRef::new("flowchart", "process");
    for (k, routing) in Routing::ALL.iter().enumerate() {
        let x = k as f64 * 460.0;
        let a = shape(
            &mut doc,
            layer,
            process.clone(),
            Rect::new(x, top, x + 120.0, top + 60.0),
            "Source",
        );
        let b = shape(
            &mut doc,
            layer,
            process.clone(),
            Rect::new(x + 260.0, top + 140.0, x + 380.0, top + 200.0),
            "Target",
        );
        connector(
            &mut doc,
            layer,
            Endpoint::glued(a, None),
            Endpoint::glued(b, None),
            |c| {
                c.routing = *routing;
                c.start_marker = Marker::ALL[k + 4];
                c.text = routing.label().into();
            },
        );
    }
    // Every end marker.
    let y = top + 280.0;
    for (k, marker) in Marker::ALL.iter().enumerate() {
        let x = k as f64 * 170.0;
        let (a, b) = (Point::new(x, y), Point::new(x + 130.0, y));
        connector(&mut doc, layer, Endpoint::Free(a), Endpoint::Free(b), |c| {
            c.end_marker = *marker;
            c.routing = Routing::Straight;
            c.text = marker.label().into();
        });
    }
    let options = bp_export::SvgOptions {
        embed_fonts: true,
        ..Default::default()
    };
    std::fs::write(&out, bp_export::page_to_svg(&doc, page, &options)).unwrap();
    println!("Wrote {out}");
}
