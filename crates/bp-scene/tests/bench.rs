//! Performance budgets from the project plan, on a 5,000-element page.
//!
//! Timings are only checked in release builds (`cargo test --release -p
//! bp-scene --test bench -- --nocapture`); debug builds just run the code.

use bp_model::kurbo::{Point, Rect, Vec2};
use bp_model::{Document, Element, ElementId, ElementKind, Endpoint, Parent, ShapeRef};
use bp_scene::SceneCache;
use bp_shapes::Libraries;
use std::time::{Duration, Instant};

const SHAPES: [(&str, &str); 4] = [
    ("flowchart", "process"),
    ("flowchart", "decision"),
    ("flowchart", "document"),
    ("basic", "ellipse"),
];

/// A grid of 2,500 shapes with text, each joined to its right and lower
/// neighbours: 2,500 shapes and about 2,450 connectors. One hub shape in
/// the middle also has 50 connectors to shapes around it.
fn big_page() -> (Document, ElementId) {
    let mut doc = Document::new();
    let page = doc.first_page().unwrap();
    let layer = Parent::Layer(doc.layers_of(page)[0].id);
    // Debug builds run unoptimised and only exercise the code: keep it small.
    let n = if cfg!(debug_assertions) { 16 } else { 50 };
    let mut ids = Vec::new();
    for row in 0..n {
        for col in 0..n {
            let (library, name) = SHAPES[(row + col) % SHAPES.len()];
            let at = Point::new(col as f64 * 200.0, row as f64 * 150.0);
            let order = doc.next_order_key(layer);
            let mut el = Element::shape(
                ShapeRef::new(library, name),
                layer,
                order,
                Rect::from_center_size(at, (120.0, 60.0)),
            );
            if let ElementKind::Shape(s) = &mut el.kind {
                s.text = format!("Step {row}-{col} validates the order");
            }
            ids.push(el.id);
            doc.elements.insert(el.id, el);
        }
    }
    let connect = |doc: &mut Document, a: ElementId, b: ElementId| {
        let order = doc.next_order_key(layer);
        let el = Element::connector(
            Endpoint::glued(a, None),
            Endpoint::glued(b, None),
            layer,
            order,
        );
        doc.elements.insert(el.id, el);
    };
    let hub = ids[n / 2 * n + n / 2];
    let mut count = 0;
    for row in 0..n {
        for col in 0..n {
            let id = ids[row * n + col];
            if col + 1 < n && count < n * n - 50 {
                connect(&mut doc, id, ids[row * n + col + 1]);
                count += 1;
            }
            if row + 1 < n && count < n * n - 50 && (row + col) % 2 == 0 {
                connect(&mut doc, id, ids[(row + 1) * n + col]);
                count += 1;
            }
        }
    }
    for k in 0..50 {
        let other = ids[(k / 10 + n / 2 - 5) * n + n / 2 - 5 + k % 10];
        if other != hub {
            connect(&mut doc, hub, other);
        }
    }
    (doc, hub)
}

fn time<T>(f: impl FnOnce() -> T) -> (T, Duration) {
    let start = Instant::now();
    let out = f();
    (out, start.elapsed())
}

#[test]
fn five_thousand_elements_stay_within_budget() {
    let (mut doc, hub) = big_page();
    let page = doc.first_page().unwrap();
    let libs = Libraries::builtin();
    if !cfg!(debug_assertions) {
        assert!(
            doc.elements.len() >= 5000,
            "{} elements",
            doc.elements.len()
        );
    }

    let mut cache = SceneCache::default();
    let (scene, cold) = time(|| cache.build(&doc, page, libs));
    assert_eq!(scene.order.len(), doc.elements.len());
    // As the app does: drop the old scene before the next build. Each
    // measurement is the best of several runs, to keep scheduler noise
    // out of the numbers.
    drop(scene);
    // Debug builds only exercise the code; one run is enough.
    let runs = if cfg!(debug_assertions) { 1 } else { 10 };
    let mut warm = Duration::MAX;
    for _ in 0..runs {
        let (scene, t) = time(|| cache.build(&doc, page, libs));
        assert_eq!(cache.rebuilt, 0);
        drop(scene);
        warm = warm.min(t);
    }

    // Frames of dragging the hub, which has 50+ connectors.
    let mut drag = Duration::MAX;
    let mut rebuilt = 0;
    for _ in 0..runs {
        if let ElementKind::Shape(s) = &mut doc.elements.get_mut(&hub).unwrap().kind {
            s.bounds = s.bounds + Vec2::new(7.0, 3.0);
        }
        let (scene, t) = time(|| cache.build(&doc, page, libs));
        drop(scene);
        rebuilt = cache.rebuilt;
        drag = drag.min(t);
    }
    assert!(
        rebuilt > 50,
        "the hub and its connectors rebuilt: {rebuilt}"
    );
    let scene = cache.build(&doc, page, libs);

    let points: Vec<Point> = (0..1000)
        .map(|i| Point::new((i * 97 % 10_000) as f64, (i * 61 % 7_500) as f64))
        .collect();
    let (hits, hit_time) = time(|| {
        points
            .iter()
            .filter(|p| scene.hit(**p, 3.0, |_| true).is_some())
            .count()
    });
    assert!(hits > 0);

    println!(
        "{} elements: cold build {cold:?}, warm rebuild {warm:?}, drag frame {drag:?} ({} rebuilt), 1000 hit tests {hit_time:?}",
        doc.elements.len(),
        rebuilt
    );
    if cfg!(debug_assertions) {
        return;
    }
    assert!(
        cold < Duration::from_millis(1500),
        "cold build took {cold:?}"
    );
    assert!(
        warm < Duration::from_millis(16),
        "warm rebuild took {warm:?}"
    );
    assert!(
        drag < Duration::from_millis(16),
        "a drag frame took {drag:?}"
    );
    assert!(
        hit_time < Duration::from_millis(50),
        "hit tests took {hit_time:?}"
    );
}
