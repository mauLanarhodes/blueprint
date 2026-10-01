//! The Phase 0 gate, end to end: draw, save, reopen and export an SVG,
//! using the same commands the app uses and the real CLI binary.

use bp_commands::{Command, History};
use bp_model::{Color, Document, Element, ShapeKind};
use kurbo::Rect;
use std::process::Command as Process;

#[test]
fn draw_save_reopen_export() {
    // Draw: four shapes and a label through the undo history.
    let mut doc = Document::new();
    let mut history = History::new();
    let page = doc.first_page().unwrap();
    let layer = doc.layers_of(page)[0].id;
    let shapes = [
        (
            ShapeKind::RoundedRectangle,
            Rect::new(0.0, 0.0, 160.0, 70.0),
            "Web app",
        ),
        (
            ShapeKind::Diamond,
            Rect::new(220.0, -10.0, 340.0, 80.0),
            "Signed in?",
        ),
        (
            ShapeKind::Rectangle,
            Rect::new(400.0, 0.0, 560.0, 70.0),
            "Orders API",
        ),
        (
            ShapeKind::Ellipse,
            Rect::new(400.0, 140.0, 560.0, 220.0),
            "Postgres",
        ),
        (
            ShapeKind::Text,
            Rect::new(0.0, 140.0, 200.0, 170.0),
            "Phase 0 gate",
        ),
    ];
    for (kind, bounds, text) in shapes {
        let mut el = Element::new(kind, layer, doc.next_order_key(layer), bounds);
        el.text = text.into();
        if kind == ShapeKind::Ellipse {
            el.style.fill = Some(Color::rgb(0xdb, 0xea, 0xfe));
        }
        history
            .apply(&mut doc, "Add", [Command::Insert(Box::new(el))])
            .unwrap();
    }
    assert_eq!(doc.elements.len(), 5);

    // Save and reopen.
    let dir = std::env::temp_dir().join(format!("bp-gate-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("gate.blueprint");
    bp_io::save(&doc, &file).unwrap();
    let reopened = bp_io::load(&file).unwrap();
    assert_eq!(reopened, doc);

    // Export through the real CLI binary.
    let svg_path = dir.join("gate.svg");
    let status = Process::new(env!("CARGO_BIN_EXE_blueprint-cli"))
        .args(["export", file.to_str().unwrap(), svg_path.to_str().unwrap()])
        .status()
        .unwrap();
    assert!(status.success());
    let svg = std::fs::read_to_string(&svg_path).unwrap();
    assert_eq!(svg.matches("<path").count(), 4, "four outlined shapes");
    for text in [
        "Web app",
        "Signed in?",
        "Orders API",
        "Postgres",
        "Phase 0 gate",
    ] {
        assert!(svg.contains(&format!(">{text}</text>")), "missing {text}");
    }
    assert!(svg.contains(r##"fill="#dbeafe""##));

    // Keep the files for inspection when asked to.
    if std::env::var_os("BP_KEEP_GATE_FILES").is_some() {
        println!("gate files kept in {}", dir.display());
    } else {
        std::fs::remove_dir_all(dir).unwrap();
    }
}