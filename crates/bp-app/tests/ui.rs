//! UI tests: drive the real app with simulated mouse and keyboard input.

use bp_app::{BlueprintApp, Tool};
use bp_model::kurbo::{Point, Rect};
use bp_model::{ElementId, Endpoint, ShapeRef};
use egui::{Event, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

type Ui = Harness<'static, Option<BlueprintApp>>;

fn harness() -> Ui {
    let mut h = Harness::builder()
        .with_size(egui::vec2(1360.0, 860.0))
        // Real frame times, so double-clicks register.
        .with_step_dt(1.0 / 60.0)
        .build_ui_state(
            |ui, app: &mut Option<BlueprintApp>| {
                let app = app.get_or_insert_with(|| BlueprintApp::new(ui.ctx(), None));
                app.frame(ui);
            },
            None,
        );
    h.run();
    h
}

fn app(h: &mut Ui) -> &mut BlueprintApp {
    h.state_mut()
        .as_mut()
        .expect("app created on the first frame")
}

/// The screen position of a page point.
fn screen(h: &mut Ui, p: Point) -> Pos2 {
    let a = app(h);
    a.view.to_screen(a.canvas_rect.min, p)
}

fn click(h: &mut Ui, at: Pos2) {
    h.hover_at(at);
    h.drag_at(at);
    h.event(Event::PointerButton {
        pos: at,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run();
}

/// Lets half a second pass, so the next click is not part of a
/// double-click.
fn wait(h: &mut Ui) {
    h.run_steps(30);
}

/// Two clicks in quick succession (one frame per event, so well inside
/// the double-click interval).
fn double_click(h: &mut Ui, at: Pos2) {
    h.hover_at(at);
    for pressed in [true, false, true, false] {
        h.event(Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
    }
    h.step();
    h.run();
}

fn drag(h: &mut Ui, from: Pos2, to: Pos2) {
    h.hover_at(from);
    h.drag_at(from);
    for i in 1..=8 {
        let t = i as f32 / 8.0;
        h.hover_at(from + (to - from) * t);
    }
    h.event(Event::PointerButton {
        pos: to,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run();
}

fn bounds(h: &mut Ui, id: ElementId) -> Rect {
    app(h).doc.elements[&id].as_shape().unwrap().bounds
}

fn add_shape(h: &mut Ui, name: &str, bounds: Rect) -> ElementId {
    let (library, shape) = name.split_once('/').unwrap();
    let id = app(h)
        .insert_shape(ShapeRef::new(library, shape), bounds)
        .unwrap();
    app(h).selection.clear();
    h.run();
    id
}

fn shapes(h: &mut Ui) -> Vec<ElementId> {
    let a = app(h);
    a.doc
        .elements
        .values()
        .filter(|e| e.is_shape())
        .map(|e| e.id)
        .collect()
}

#[test]
fn draw_two_shapes_and_connect_them_with_the_mouse() {
    let mut h = harness();
    h.key_press(Key::R);
    h.run();
    assert!(matches!(app(&mut h).tool, Tool::Shape(_)));
    let at = screen(&mut h, Point::new(150.0, 150.0));
    click(&mut h, at);
    assert_eq!(
        app(&mut h).tool,
        Tool::Select,
        "back to select after placing"
    );
    h.key_press(Key::R);
    h.run();
    let at = screen(&mut h, Point::new(500.0, 150.0));
    click(&mut h, at);
    let ids = shapes(&mut h);
    assert_eq!(ids.len(), 2);
    let (a, b) = if bounds(&mut h, ids[0]).x0 < bounds(&mut h, ids[1]).x0 {
        (ids[0], ids[1])
    } else {
        (ids[1], ids[0])
    };

    // Deselect, hover the first shape to show its ports, then drag from
    // its east port onto the second shape.
    let empty = screen(&mut h, Point::new(300.0, 500.0));
    click(&mut h, empty);
    assert!(app(&mut h).selection.is_empty());
    let east = {
        let a_geom = app(&mut h).scene.shape(a).unwrap().clone();
        a_geom
            .ports
            .iter()
            .find(|p| p.id.as_str() == "e")
            .unwrap()
            .at
    };
    let from = screen(&mut h, east);
    let center = bounds(&mut h, b).center();
    let to = screen(&mut h, center);
    h.hover_at(from);
    h.run();
    drag(&mut h, from, to);

    let doc = &app(&mut h).doc;
    let connectors: Vec<_> = doc
        .elements
        .values()
        .filter_map(|e| e.as_connector())
        .collect();
    assert_eq!(connectors.len(), 1, "a connector was drawn");
    let c = connectors[0];
    assert_eq!(c.source, Endpoint::glued(a, Some("e")));
    assert_eq!(c.target.element(), Some(b), "glued to the second shape");

    // Undo removes it; redo brings it back.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert!(app(&mut h).doc.elements.values().all(|e| !e.is_connector()));
    h.key_press_modifiers(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::Z);
    h.run();
    assert_eq!(
        app(&mut h)
            .doc
            .elements
            .values()
            .filter(|e| e.is_connector())
            .count(),
        1
    );
}

#[test]
fn dragging_snaps_to_smart_guides() {
    let mut h = harness();
    let a = add_shape(
        &mut h,
        "basic/rectangle",
        Rect::new(100.0, 100.0, 240.0, 170.0),
    );
    let b = add_shape(
        &mut h,
        "basic/rectangle",
        Rect::new(400.0, 300.0, 540.0, 370.0),
    );
    // Drag B so its top lands 3 units below A's top: it snaps level.
    let from = screen(&mut h, Point::new(470.0, 335.0));
    let to = screen(&mut h, Point::new(473.0, 138.0));
    drag(&mut h, from, to);
    assert_eq!(bounds(&mut h, b).y0, bounds(&mut h, a).y0);
    assert!(app(&mut h).guides.is_empty(), "guides clear after the drag");
    // The whole drag is one undo step.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!(bounds(&mut h, b), Rect::new(400.0, 300.0, 540.0, 370.0));
}

#[test]
fn marquee_selects_what_it_encloses() {
    let mut h = harness();
    let a = add_shape(
        &mut h,
        "basic/rectangle",
        Rect::new(100.0, 100.0, 160.0, 140.0),
    );
    let b = add_shape(
        &mut h,
        "basic/ellipse",
        Rect::new(200.0, 100.0, 260.0, 140.0),
    );
    add_shape(
        &mut h,
        "basic/rectangle",
        Rect::new(500.0, 100.0, 560.0, 140.0),
    );
    let from = screen(&mut h, Point::new(80.0, 80.0));
    let to = screen(&mut h, Point::new(300.0, 200.0));
    drag(&mut h, from, to);
    let mut selected = app(&mut h).selection.clone();
    selected.sort();
    let mut expected = vec![a, b];
    expected.sort();
    assert_eq!(selected, expected);
}

#[test]
fn resizing_from_a_corner_handle() {
    let mut h = harness();
    let id = add_shape(
        &mut h,
        "basic/rectangle",
        Rect::new(100.0, 100.0, 200.0, 160.0),
    );
    app(&mut h).selection = vec![id];
    h.run();
    let from = screen(&mut h, Point::new(200.0, 160.0));
    let to = screen(&mut h, Point::new(260.0, 200.0));
    drag(&mut h, from, to);
    assert_eq!(bounds(&mut h, id), Rect::new(100.0, 100.0, 260.0, 200.0));
}

#[test]
fn double_click_edits_text_in_place() {
    let mut h = harness();
    let id = add_shape(
        &mut h,
        "flowchart/process",
        Rect::new(100.0, 100.0, 220.0, 160.0),
    );
    let at = screen(&mut h, Point::new(160.0, 130.0));
    double_click(&mut h, at);
    assert_eq!(app(&mut h).editing.as_ref().map(|e| e.id), Some(id));
    h.run();
    h.event(Event::Text("Ship order".into()));
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::Enter);
    h.run();
    assert!(app(&mut h).editing.is_none());
    assert_eq!(app(&mut h).doc.elements[&id].text(), Some("Ship order"));
}

#[test]
fn quick_insert_places_a_shape_by_name() {
    let mut h = harness();
    let at = screen(&mut h, Point::new(300.0, 300.0));
    h.hover_at(at);
    h.run();
    h.event(Event::Text("/".into()));
    h.run();
    assert!(app(&mut h).quick_insert.is_some());
    h.event(Event::Text("decision".into()));
    h.run();
    h.key_press(Key::Enter);
    h.run();
    let doc = &app(&mut h).doc;
    let shape = doc
        .elements
        .values()
        .find_map(|e| e.as_shape())
        .expect("inserted");
    assert_eq!(shape.shape, ShapeRef::new("flowchart", "decision"));
    assert!(
        shape.bounds.contains(Point::new(300.0, 300.0)),
        "at the pointer"
    );
}

#[test]
fn palette_click_inserts_a_shape() {
    let mut h = harness();
    h.get_by_label("Predefined process").click();
    h.run();
    let doc = &app(&mut h).doc;
    let shape = doc
        .elements
        .values()
        .find_map(|e| e.as_shape())
        .expect("inserted");
    assert_eq!(
        shape.shape,
        ShapeRef::new("flowchart", "predefined-process")
    );
    assert_eq!(app(&mut h).palette.recent.len(), 1);
}

#[test]
fn groups_select_together_and_open_on_double_click() {
    let mut h = harness();
    let a = add_shape(
        &mut h,
        "basic/rectangle",
        Rect::new(100.0, 100.0, 160.0, 140.0),
    );
    let b = add_shape(
        &mut h,
        "basic/rectangle",
        Rect::new(200.0, 100.0, 260.0, 140.0),
    );
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::G);
    h.run();
    let group = app(&mut h).selection[0];
    assert_eq!(app(&mut h).doc.descendants(group), vec![a, b]);

    // A click on a member selects the whole group...
    let empty = screen(&mut h, Point::new(400.0, 400.0));
    click(&mut h, empty);
    let on_a = screen(&mut h, Point::new(130.0, 120.0));
    click(&mut h, on_a);
    assert_eq!(app(&mut h).selection, vec![group]);
    // ...a double-click enters it and edits the member.
    wait(&mut h);
    double_click(&mut h, on_a);
    assert_eq!(app(&mut h).scope, Some(group));
    assert_eq!(app(&mut h).selection, vec![a]);
    h.key_press(Key::Escape);
    h.run();
    h.key_press(Key::Escape);
    h.run();
    h.key_press(Key::Escape);
    h.run();
    assert_eq!(app(&mut h).scope, None, "escape backs out of the group");
}

#[test]
fn duplicate_and_delete_keep_connectors_consistent() {
    let mut h = harness();
    let a = add_shape(
        &mut h,
        "basic/rectangle",
        Rect::new(100.0, 100.0, 160.0, 140.0),
    );
    let b = add_shape(
        &mut h,
        "basic/rectangle",
        Rect::new(300.0, 100.0, 360.0, 140.0),
    );
    app(&mut h).insert_connector(Endpoint::glued(a, None), Endpoint::glued(b, None));
    app(&mut h).selection = vec![a, b];
    h.key_press_modifiers(Modifiers::COMMAND, Key::D);
    h.run();
    // Two more shapes and the connector between them.
    assert_eq!(app(&mut h).doc.elements.len(), 6);
    assert_eq!(app(&mut h).selection.len(), 3);
    // Deleting the original shapes takes their connector too.
    app(&mut h).selection = vec![a];
    h.key_press(Key::Delete);
    h.run();
    assert_eq!(app(&mut h).doc.elements.len(), 4);
    assert_eq!(app(&mut h).doc.validate(), Ok(()));
}

#[test]
fn pages_and_layers_change_what_is_shown() {
    let mut h = harness();
    add_shape(
        &mut h,
        "basic/rectangle",
        Rect::new(100.0, 100.0, 160.0, 140.0),
    );
    app(&mut h).add_page();
    h.run();
    assert_eq!(app(&mut h).doc.pages.len(), 2);
    assert!(app(&mut h).scene.order.is_empty(), "the new page is empty");
    h.key_press_modifiers(Modifiers::COMMAND, Key::PageUp);
    h.run();
    assert_eq!(app(&mut h).scene.order.len(), 1);
    let layer = app(&mut h).layer;
    app(&mut h).set_layer_flag(layer, bp_commands::LayerProp::Visible(false));
    h.run();
    assert!(
        app(&mut h).scene.order.is_empty(),
        "hidden layers draw nothing"
    );
}

#[test]
fn text_boxes_fit_their_text_in_one_undo_step() {
    let mut h = harness();
    let id = add_shape(&mut h, "basic/text", Rect::new(100.0, 100.0, 220.0, 132.0));
    let before = bounds(&mut h, id);
    let at = screen(&mut h, Point::new(160.0, 116.0));
    double_click(&mut h, at);
    assert_eq!(app(&mut h).editing.as_ref().map(|e| e.id), Some(id));
    h.event(Event::Text(
        "A note long enough to wrap onto several lines".into(),
    ));
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::Enter);
    h.run();
    assert!(
        bounds(&mut h, id).height() > before.height(),
        "the box grew to fit"
    );
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!(app(&mut h).doc.elements[&id].text(), Some(""));
    assert_eq!(bounds(&mut h, id), before, "text and size undo together");
}
