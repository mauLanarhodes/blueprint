//! The Phase 1 gate, end to end: build a two-page flowchart with glued,
//! routed connectors, groups and layers through the same commands the app
//! uses; save, reopen and export it with the real CLI binary.

use bp_commands::edit;
use bp_commands::{Command, History, LayerProp, Prop};
use bp_model::kurbo::{Rect, Vec2};
use bp_model::{
    Dash, Document, Element, ElementId, Endpoint, Layer, Marker, OrderKey, Page, PageId, Parent,
    Routing, ShapeRef,
};
use std::path::Path;
use std::process::Command as Process;

struct Builder {
    doc: Document,
    history: History,
}

impl Builder {
    fn run(&mut self, label: &str, commands: Vec<Command>) {
        self.history.apply(&mut self.doc, label, commands).unwrap();
        assert_eq!(self.doc.validate(), Ok(()), "after {label}");
    }

    fn shape(&mut self, layer: Parent, shape: &str, at: (f64, f64), text: &str) -> ElementId {
        let reference = ShapeRef::parse(shape).unwrap();
        let def = bp_shapes::Libraries::builtin().get(&reference).unwrap();
        let bounds = Rect::from_center_size(at, def.default_size);
        let order = self.doc.next_order_key(layer);
        let mut el = Element::shape(reference, layer, order, bounds);
        if let Some(s) = el.as_shape_mut() {
            s.text = text.into();
        }
        let id = el.id;
        self.run("Add shape", vec![Command::Insert(Box::new(el))]);
        id
    }

    fn connect(&mut self, layer: Parent, from: Endpoint, to: Endpoint, label: &str) -> ElementId {
        let order = self.doc.next_order_key(layer);
        let mut el = Element::connector(from, to, layer, order);
        if let Some(c) = el.as_connector_mut() {
            c.text = label.into();
        }
        let id = el.id;
        self.run("Connect", vec![Command::Insert(Box::new(el))]);
        id
    }
}

fn add_page(b: &mut Builder, name: &str) -> (PageId, Parent) {
    let last = b.doc.pages_sorted().last().unwrap().order.clone();
    let page = Page::new(name, OrderKey::after(&last));
    let layer = Layer::new(page.id, "Layer 1", OrderKey::first());
    let ids = (page.id, Parent::Layer(layer.id));
    b.run(
        "Add page",
        vec![
            Command::InsertPage(Box::new(page)),
            Command::InsertLayer(Box::new(layer)),
        ],
    );
    ids
}

fn cli(args: &[&str]) -> String {
    let out = Process::new(env!("CARGO_BIN_EXE_blueprint-cli"))
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

fn path_str(p: &Path) -> &str {
    p.to_str().unwrap()
}

#[test]
fn flowchart_save_reopen_export() {
    let mut b = Builder {
        doc: Document::new(),
        history: History::new(),
    };
    let first = b.doc.first_page().unwrap();
    let main = Parent::Layer(b.doc.layers_of(first)[0].id);
    b.run(
        "Rename page",
        vec![Command::SetPage {
            id: first,
            prop: bp_commands::PageProp::Name("Checkout".into()),
        }],
    );

    // Page 1: a checkout flow.
    let start = b.shape(main, "flowchart/terminator", (100.0, 50.0), "Start");
    let validate = b.shape(
        main,
        "flowchart/process",
        (100.0, 160.0),
        "Validate cart and reserve stock",
    );
    let stock = b.shape(main, "flowchart/decision", (100.0, 290.0), "In stock?");
    let pay = b.shape(main, "flowchart/process", (100.0, 420.0), "Take payment");
    let confirm = b.shape(
        main,
        "flowchart/document",
        (100.0, 540.0),
        "Send confirmation",
    );
    let notify = b.shape(
        main,
        "flowchart/manual-operation",
        (340.0, 290.0),
        "Notify customer",
    );
    let end = b.shape(main, "flowchart/terminator", (340.0, 640.0), "End");
    let note = b.shape(
        main,
        "flowchart/annotation",
        (380.0, 160.0),
        "Holds stock for 15 minutes",
    );

    let glue = |id, port: Option<&str>| Endpoint::glued(id, port);
    b.connect(main, glue(start, Some("s")), glue(validate, Some("n")), "");
    b.connect(main, glue(validate, None), glue(stock, None), "");
    let yes = b.connect(main, glue(stock, Some("s")), glue(pay, Some("n")), "Yes");
    let no = b.connect(main, glue(stock, Some("e")), glue(notify, Some("w")), "No");
    b.connect(main, glue(pay, None), glue(confirm, None), "");
    b.connect(main, glue(confirm, None), glue(end, None), "");
    b.connect(main, glue(notify, None), glue(end, None), "");
    let dashed = b.connect(main, glue(validate, Some("e")), glue(note, Some("w")), "");
    b.run(
        "Style annotation link",
        vec![
            Command::Set {
                id: dashed,
                prop: Prop::Dash(Some(Dash::Dashed)),
            },
            Command::Set {
                id: dashed,
                prop: Prop::EndMarker(Marker::None),
            },
            Command::Set {
                id: dashed,
                prop: Prop::Routing(Routing::Straight),
            },
        ],
    );

    // Group payment and confirmation, then move the group: the glued
    // connectors follow because their routes are derived.
    let (group, cmds) = edit::group(&b.doc, &[pay, confirm]).unwrap();
    b.run("Group", cmds);
    b.run(
        "Move",
        edit::translate(&b.doc, &[group], Vec2::new(0.0, 20.0)),
    );
    assert_eq!(b.doc.descendants(group), vec![pay, confirm]);

    // An edit that is undone must leave no trace.
    let before = b.doc.clone();
    b.run("Delete", edit::remove(&b.doc, &[stock]));
    assert!(
        !b.doc.elements.contains_key(&yes),
        "attached connectors go too"
    );
    assert!(b.history.undo(&mut b.doc));
    assert_eq!(b.doc, before);

    // Page 2, with a hidden layer that must not export.
    let (fulfil, ship_layer) = add_page(&mut b, "Fulfilment");
    let pick = b.shape(ship_layer, "flowchart/process", (100.0, 50.0), "Pick items");
    let pack = b.shape(
        ship_layer,
        "flowchart/process",
        (100.0, 180.0),
        "Pack parcel",
    );
    b.connect(ship_layer, glue(pick, None), glue(pack, None), "");
    let draft = Layer::new(fulfil, "Drafts", OrderKey::after(&OrderKey::first()));
    let draft_id = draft.id;
    b.run("Add layer", vec![Command::InsertLayer(Box::new(draft))]);
    b.shape(
        Parent::Layer(draft_id),
        "basic/sticky-note",
        (400.0, 100.0),
        "Secret draft",
    );
    b.run(
        "Hide layer",
        vec![Command::SetLayer {
            id: draft_id,
            prop: LayerProp::Visible(false),
        }],
    );

    // Save both formats and reopen them.
    let dir = std::env::temp_dir().join(format!("bp-gate-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("checkout.blueprint");
    let json_file = dir.join("checkout.blueprint.json");
    bp_io::save(&b.doc, &file).unwrap();
    bp_io::save(&b.doc, &json_file).unwrap();
    assert_eq!(bp_io::load(&file).unwrap(), b.doc);
    assert_eq!(bp_io::load(&json_file).unwrap(), b.doc);

    // The saved file holds only what was set: no routes, no layout.
    let json = std::fs::read_to_string(&json_file).unwrap();
    assert!(!json.contains("points") && !json.contains("lines"));

    // Export page 1 through the real binary.
    let svg_path = dir.join("checkout.svg");
    cli(&["export", path_str(&file), path_str(&svg_path)]);
    let svg = std::fs::read_to_string(&svg_path).unwrap();
    // The export matches what the app draws: every line the scene lays out
    // is in the file, wrapped the same way.
    let scene = bp_scene::build_page(&b.doc, first);
    let mut lines = Vec::new();
    for item in scene.list.items() {
        if let bp_scene::Primitive::Text(run) = &item.primitive {
            lines.extend(run.lines.iter().map(|l| l.text.clone()));
        }
    }
    assert!(lines.len() > 10, "{lines:?}");
    for line in &lines {
        assert!(svg.contains(&format!(">{line}</text>")), "missing {line:?}");
    }
    for label in ["Yes", "No", "In stock?"] {
        assert!(lines.iter().any(|l| l == label), "{label}");
    }
    assert!(
        lines
            .iter()
            .filter(|l| l.contains("Validate") || l.contains("stock"))
            .count()
            >= 2,
        "long text wraps: {lines:?}"
    );
    assert!(svg.contains("stroke-dasharray"), "dashed annotation link");
    assert!(!svg.contains("Pick items"), "page 2 is not on page 1");

    let yes_route = scene.connector(yes).unwrap();
    let stock_s = scene
        .shape(stock)
        .unwrap()
        .ports
        .iter()
        .find(|p| p.id.as_str() == "s")
        .unwrap()
        .at;
    assert_eq!(
        yes_route.points[0], stock_s,
        "Yes leaves the decision's bottom port"
    );
    assert!(scene.connector(no).unwrap().label.is_some());

    // Page 2 by name, with fonts embedded; the hidden layer stays hidden.
    let page2 = dir.join("fulfilment.svg");
    cli(&[
        "export",
        path_str(&file),
        path_str(&page2),
        "--page",
        "Fulfilment",
        "--embed-fonts",
    ]);
    let svg2 = std::fs::read_to_string(&page2).unwrap();
    assert!(svg2.contains(">Pick items</text>") && svg2.contains("@font-face"));
    assert!(!svg2.contains("Secret draft"));

    let info = cli(&["info", path_str(&file)]);
    assert!(info.contains("Checkout: 8 shapes, 8 connectors"), "{info}");
    assert!(info.contains("Drafts (hidden): 1 elements"), "{info}");

    if std::env::var_os("BP_KEEP_GATE_FILES").is_some() {
        println!("gate files kept in {}", dir.display());
    } else {
        std::fs::remove_dir_all(dir).unwrap();
    }
}
