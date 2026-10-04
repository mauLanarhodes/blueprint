//! UI tests: drive the real app with simulated mouse and keyboard input.

use bp_app::{BlueprintApp, ErdConnection, Tool};
use bp_model::kurbo::{Point, Rect};
use bp_model::{ColumnId, DiagramKind, ElementId, Endpoint, Marker, PortId, ShapeRef};
use egui::{Event, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::NodeT;
use egui_kittest::kittest::Queryable;

type Ui = Harness<'static, Option<BlueprintApp>>;

fn raw_harness() -> Ui {
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

fn harness_for(kind: DiagramKind) -> Ui {
    let mut h = raw_harness();
    app(&mut h).choose_page_kind(kind);
    h.run();
    h
}

fn harness() -> Ui {
    harness_for(DiagramKind::Flowchart)
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

fn toolbar_rect(h: &Ui) -> egui::Rect {
    h.ctx
        .memory(|memory| memory.area_rect(egui::Id::new("floating_toolbar")))
        .expect("floating toolbar is visible")
}

#[test]
fn floating_toolbar_stays_centered_inside_the_canvas_after_resize() {
    for kind in [DiagramKind::Flowchart, DiagramKind::Erd] {
        let mut h = harness_for(kind);
        for size in [
            egui::vec2(1360.0, 860.0),
            egui::vec2(920.0, 650.0),
            egui::vec2(1800.0, 1000.0),
        ] {
            h.set_size(size);
            h.run();
            let toolbar = toolbar_rect(&h);
            let canvas = app(&mut h).canvas_rect;
            assert!(
                canvas.contains_rect(toolbar),
                "toolbar stays within canvas: kind={kind:?}, window={size:?}, canvas={canvas:?}, toolbar={toolbar:?}"
            );
            assert!(
                (toolbar.center().x - canvas.center().x).abs() <= 1.0,
                "toolbar is centered on the canvas rather than the window"
            );
            assert!(
                toolbar.top() > canvas.center().y,
                "toolbar is near the bottom"
            );
            assert!(
                toolbar.bottom() < canvas.bottom(),
                "toolbar floats above the edge"
            );
        }
    }
}

#[test]
fn floating_toolbar_shape_tool_places_a_shape_on_the_canvas() {
    let mut h = harness();
    h.get_by_label(egui_phosphor::regular::SQUARE).click();
    h.run();
    assert_eq!(
        app(&mut h).tool,
        Tool::Shape(ShapeRef::new("basic", "rectangle"))
    );
    assert!(
        shapes(&mut h).is_empty(),
        "tool clicks do not draw under the bar"
    );
    let point = Point::new(220.0, 180.0);
    let at = screen(&mut h, point);
    click(&mut h, at);
    let inserted = shapes(&mut h);
    assert_eq!(inserted.len(), 1);
    assert!(bounds(&mut h, inserted[0]).contains(point));
    assert_eq!(app(&mut h).tool, Tool::Select);
}

#[test]
fn floating_toolbar_padding_blocks_canvas_clicks_drags_and_scroll() {
    let mut h = harness();
    let id = add_shape(
        &mut h,
        "basic/rectangle",
        Rect::new(100.0, 100.0, 200.0, 160.0),
    );
    app(&mut h).selection = vec![id];
    app(&mut h).tool = Tool::Shape(ShapeRef::new("basic", "ellipse"));
    h.run();
    let before = app(&mut h).doc.clone();
    let view = app(&mut h).view;
    let toolbar = toolbar_rect(&h);
    // The frame's top padding is inside the bar, above its buttons.
    let padding = egui::pos2(toolbar.center().x, toolbar.top() + 4.0);
    click(&mut h, padding);
    drag(&mut h, padding, padding + egui::vec2(80.0, 0.0));
    h.hover_at(padding);
    for modifiers in [Modifiers::NONE, Modifiers::COMMAND] {
        h.event(Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, 120.0),
            phase: egui::TouchPhase::Move,
            modifiers,
        });
        h.run_steps(60);
    }
    assert_eq!(app(&mut h).doc, before);
    assert_eq!(app(&mut h).selection, vec![id]);
    assert_eq!(app(&mut h).view, view);
}

#[test]
fn floating_toolbar_rejects_palette_drops_but_canvas_accepts_them() {
    let mut h = harness();
    let source = h.get_by_label("Rectangle").rect().center();
    let toolbar = toolbar_rect(&h);
    let padding = egui::pos2(toolbar.center().x, toolbar.top() + 4.0);
    drag(&mut h, source, padding);
    assert!(
        shapes(&mut h).is_empty(),
        "toolbar is not a shape drop target: toolbar={toolbar:?}, padding={padding:?}, layer={:?}",
        h.ctx.layer_id_at(padding)
    );
    assert!(app(&mut h).palette.dragging.is_none());

    let point = Point::new(260.0, 200.0);
    let target = screen(&mut h, point);
    drag(&mut h, source, target);
    let inserted = shapes(&mut h);
    assert_eq!(inserted.len(), 1);
    assert!(bounds(&mut h, inserted[0]).contains(point));
    assert!(app(&mut h).palette.dragging.is_none());
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
    app(&mut h).add_page_with_kind(DiagramKind::Flowchart);
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
fn locked_connector_properties_cannot_be_changed_in_the_inspector() {
    let mut h = harness();
    let id = app(&mut h)
        .insert_connector(
            Endpoint::Free(Point::new(100.0, 100.0)),
            Endpoint::Free(Point::new(300.0, 200.0)),
        )
        .unwrap();
    app(&mut h).toggle_lock();
    h.run();
    assert!(h.get_by_label("Straight").accesskit_node().is_disabled());
    let before = app(&mut h).doc.clone();
    h.get_by_label("Straight").click();
    h.run();
    assert_eq!(app(&mut h).doc, before);

    h.key_press_modifiers(Modifiers::COMMAND, Key::L);
    h.run();
    assert!(!h.get_by_label("Straight").accesskit_node().is_disabled());
    h.get_by_label("Straight").click();
    h.run();
    assert_eq!(
        app(&mut h).doc.elements[&id]
            .as_connector()
            .unwrap()
            .routing,
        bp_model::Routing::Straight
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

fn table_column_port(h: &mut Ui, table: ElementId, column: ColumnId, left: bool) -> Point {
    app(h).refresh_scene();
    let port = PortId::column(column, left);
    app(h)
        .scene
        .shape(table)
        .unwrap()
        .ports
        .iter()
        .find(|p| p.id == port)
        .unwrap()
        .at
}

#[test]
fn erd_palette_and_inspector_add_columns_with_enter_and_undo() {
    let mut h = harness_for(DiagramKind::Erd);
    app(&mut h).palette.query = "table".into();
    h.run();
    h.get_by_label("Table").click();
    h.run();
    let id = app(&mut h).selection[0];
    let table = app(&mut h).doc.elements[&id]
        .as_shape()
        .unwrap()
        .erd
        .as_ref()
        .unwrap();
    assert_eq!(table.columns.len(), 1);
    assert!(table.columns[0].primary_key);

    h.get_by_label("Add column").click();
    h.run();
    assert_eq!(
        app(&mut h).doc.elements[&id]
            .as_shape()
            .unwrap()
            .erd
            .as_ref()
            .unwrap()
            .columns
            .len(),
        2
    );
    h.key_press(Key::Enter);
    h.run();
    assert_eq!(
        app(&mut h).doc.elements[&id]
            .as_shape()
            .unwrap()
            .erd
            .as_ref()
            .unwrap()
            .columns
            .len(),
        3
    );
    let empty = screen(&mut h, Point::new(30.0, 450.0));
    click(&mut h, empty);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!(
        app(&mut h).doc.elements[&id]
            .as_shape()
            .unwrap()
            .erd
            .as_ref()
            .unwrap()
            .columns
            .len(),
        2
    );
    assert_eq!(app(&mut h).doc.validate(), Ok(()));
}

#[test]
fn erd_column_relationships_set_fk_and_cardinalities_in_one_mouse_edit() {
    let mut h = harness_for(DiagramKind::Erd);
    let orders = add_shape(&mut h, "erd/table", Rect::new(100.0, 100.0, 380.0, 200.0));
    let customers = add_shape(&mut h, "erd/table", Rect::new(600.0, 100.0, 880.0, 200.0));
    let pk = app(&mut h).doc.elements[&customers]
        .as_shape()
        .unwrap()
        .erd
        .as_ref()
        .unwrap()
        .columns[0]
        .id;
    let fk = app(&mut h).add_table_column(orders).unwrap();
    app(&mut h).set_table_column(
        orders,
        fk,
        bp_commands::ColumnProp::Name("customer_id".into()),
    );
    app(&mut h).set_table_column(orders, fk, bp_commands::ColumnProp::Nullable(false));
    app(&mut h).selection = vec![orders];
    h.run();
    let original = app(&mut h).doc.clone();
    let source = table_column_port(&mut h, orders, fk, false);
    let target = table_column_port(&mut h, customers, pk, true);
    let from = screen(&mut h, source);
    let to = screen(&mut h, target);
    h.hover_at(from);
    h.run();
    drag(&mut h, from, to);

    let id = app(&mut h).selection[0];
    let connector = app(&mut h).doc.elements[&id]
        .as_connector()
        .expect("mouse created relationship");
    assert_eq!(
        connector.source,
        Endpoint::glued(orders, Some(PortId::column(fk, false).as_str()))
    );
    assert_eq!(
        connector.target,
        Endpoint::glued(customers, Some(PortId::column(pk, true).as_str()))
    );
    assert_eq!(
        (connector.start_marker, connector.end_marker),
        (Marker::ZeroOrMany, Marker::ExactlyOne)
    );
    assert!(
        app(&mut h).doc.elements[&orders]
            .as_shape()
            .unwrap()
            .erd
            .as_ref()
            .unwrap()
            .column(fk)
            .unwrap()
            .foreign_key
    );
    app(&mut h).undo();
    assert_eq!(
        app(&mut h).doc,
        original,
        "one undo removes the relationship and FK flag"
    );
    app(&mut h).redo();
    assert_eq!(app(&mut h).doc.validate(), Ok(()));

    // The stable endpoint follows its table after an ordinary nudge.
    app(&mut h).selection = vec![orders];
    app(&mut h).nudge(20.0, 0.0);
    h.run();
    let moved = table_column_port(&mut h, orders, fk, false);
    assert_eq!(app(&mut h).scene.connector(id).unwrap().points[0], moved);
    assert_eq!(moved.x, source.x + 20.0);
}

#[test]
fn locked_erd_tables_refuse_row_edits() {
    let mut h = harness_for(DiagramKind::Erd);
    let table = add_shape(&mut h, "erd/table", Rect::new(100.0, 100.0, 380.0, 260.0));
    app(&mut h).selection = vec![table];
    app(&mut h).toggle_lock();
    h.run();
    let before = app(&mut h).doc.clone();
    let column = app(&mut h).doc.elements[&table]
        .as_shape()
        .unwrap()
        .erd
        .as_ref()
        .unwrap()
        .columns[0]
        .id;
    assert!(h.get_by_label("Add column").accesskit_node().is_disabled());
    h.get_by_label("Add column").click();
    h.run();
    assert!(app(&mut h).add_table_column(table).is_none());
    app(&mut h).set_table_column(
        table,
        column,
        bp_commands::ColumnProp::Name("changed".into()),
    );
    app(&mut h).delete_table_column(table, column);
    assert_eq!(app(&mut h).doc, before);
}

#[test]
fn erd_mouse_connections_support_self_referencing_columns() {
    let mut h = harness_for(DiagramKind::Erd);
    let table = add_shape(&mut h, "erd/table", Rect::new(100.0, 100.0, 380.0, 200.0));
    let pk = app(&mut h).doc.elements[&table]
        .as_shape()
        .unwrap()
        .erd
        .as_ref()
        .unwrap()
        .columns[0]
        .id;
    let fk = app(&mut h).add_table_column(table).unwrap();
    app(&mut h).selection = vec![table];
    h.run();
    let before = app(&mut h).doc.clone();
    let source = table_column_port(&mut h, table, fk, false);
    let target = table_column_port(&mut h, table, pk, false);
    let from = screen(&mut h, source);
    let to = screen(&mut h, target);
    drag(&mut h, from, to);
    let id = app(&mut h).selection[0];
    let connector = app(&mut h).doc.elements[&id].as_connector().unwrap();
    assert_eq!(
        connector.source,
        Endpoint::glued(table, Some(PortId::column(fk, false).as_str()))
    );
    assert_eq!(
        connector.target,
        Endpoint::glued(table, Some(PortId::column(pk, false).as_str()))
    );
    assert_eq!(app(&mut h).doc.validate(), Ok(()));
    app(&mut h).undo();
    assert_eq!(app(&mut h).doc, before);
}

#[test]
fn erd_primary_key_port_drags_win_over_nearby_side_ports_and_resize_handles() {
    let mut h = harness_for(DiagramKind::Erd);
    let a = add_shape(&mut h, "erd/table", Rect::new(100.0, 100.0, 380.0, 200.0));
    let b = add_shape(&mut h, "erd/table", Rect::new(600.0, 100.0, 880.0, 200.0));
    let pk = |h: &mut Ui, id| {
        app(h).doc.elements[&id]
            .as_shape()
            .unwrap()
            .erd
            .as_ref()
            .unwrap()
            .columns[0]
            .id
    };
    let source = pk(&mut h, a);
    let target = pk(&mut h, b);
    app(&mut h).selection = vec![a];
    h.run();
    let from = table_column_port(&mut h, a, source, false);
    let to = table_column_port(&mut h, b, target, true);
    let from = screen(&mut h, from);
    let to = screen(&mut h, to);
    drag(&mut h, from, to);
    let id = app(&mut h).selection[0];
    let connector = app(&mut h).doc.elements[&id].as_connector().unwrap();
    assert_eq!(
        connector.source,
        Endpoint::glued(a, Some(PortId::column(source, false).as_str()))
    );
    assert_eq!(
        connector.target,
        Endpoint::glued(b, Some(PortId::column(target, true).as_str()))
    );
    assert_eq!(connector.style.dash, Some(bp_model::Dash::Solid));
}

#[test]
fn diagram_choice_sets_each_page_and_cancel_leaves_the_document_untouched() {
    let mut h = raw_harness();
    assert_eq!(app(&mut h).page_kind(), None);
    h.key_press(Key::C);
    h.run();
    assert_eq!(app(&mut h).tool, Tool::Select, "choose a page type first");
    h.get_by_label("Flowchart").click();
    h.run();
    let flowchart = app(&mut h).page;
    assert_eq!(app(&mut h).page_kind(), Some(DiagramKind::Flowchart));

    let before = app(&mut h).doc.clone();
    app(&mut h).request_add_page();
    h.run();
    h.get_by_label("Cancel").click();
    h.run();
    assert_eq!(app(&mut h).doc, before);
    assert_eq!(app(&mut h).page, flowchart);

    app(&mut h).request_add_page();
    h.run();
    h.get_by_label("ERD").click();
    h.run();
    let erd = app(&mut h).page;
    assert_ne!(erd, flowchart);
    assert_eq!(app(&mut h).doc.pages.len(), 2);
    assert_eq!(app(&mut h).page_kind(), Some(DiagramKind::Erd));
    app(&mut h).duplicate_page(erd);
    h.run();
    assert_eq!(app(&mut h).page_kind(), Some(DiagramKind::Erd));
    assert_eq!(app(&mut h).doc.pages.len(), 3);

    app(&mut h).set_page(flowchart);
    h.run();
    assert_eq!(app(&mut h).page_kind(), Some(DiagramKind::Flowchart));
    assert!(app(&mut h).page_choice.is_none());
}

#[test]
fn page_kind_filters_shapes_search_recent_items_and_quick_insert() {
    let mut h = harness();
    assert!(h.query_by_label("Table").is_none());
    assert!(h.query_by_label("Predefined process").is_some());
    assert!(h.query_all_by_label("Rectangle").next().is_some());
    h.get_by_label("Predefined process").click();
    h.run();
    let flowchart = app(&mut h).page;
    assert_eq!(app(&mut h).palette.recent.len(), 1);
    app(&mut h).add_page_with_kind(DiagramKind::Erd);
    h.run();
    let erd = app(&mut h).page;
    assert!(h.query_by_label("Table").is_some());
    assert!(h.query_by_label("Predefined process").is_none());
    assert!(h.query_all_by_label("Rectangle").next().is_some());
    assert!(h.query_by_label("Zero or many").is_some());

    app(&mut h).palette.query = "predefined".into();
    h.run();
    assert!(h.query_by_label("Predefined process").is_none());
    assert!(h.query_by_label("No shapes match").is_some());
    app(&mut h).palette.query.clear();
    app(&mut h).set_page(flowchart);
    h.run();
    assert!(h.query_by_label("Table").is_none());
    assert!(h.query_by_label("Zero or many").is_none());

    app(&mut h).palette.query = "table".into();
    h.run();
    assert!(h.query_by_label("Table").is_none());
    app(&mut h).palette.query.clear();
    app(&mut h).open_quick_insert();
    app(&mut h).quick_insert.as_mut().unwrap().query = "table".into();
    h.run();
    let before = app(&mut h).doc.clone();
    h.key_press(Key::Enter);
    h.run();
    assert!(
        app(&mut h)
            .doc
            .elements
            .values()
            .filter(|element| !before.elements.contains_key(&element.id))
            .filter_map(|element| element.as_shape())
            .all(|shape| matches!(shape.shape.library(), "basic" | "flowchart")),
        "fuzzy search only inserts shapes available to the flowchart page"
    );
    h.key_press(Key::Escape);
    h.run();

    app(&mut h).set_page(erd);
    app(&mut h).open_quick_insert();
    app(&mut h).quick_insert.as_mut().unwrap().query = "table".into();
    h.run();
    h.key_press(Key::Enter);
    h.run();
    let id = app(&mut h).selection[0];
    assert_eq!(
        app(&mut h).doc.elements[&id].as_shape().unwrap().shape,
        ShapeRef::new("erd", "table")
    );
}

#[test]
fn erd_palette_presets_draw_the_chosen_markers_and_update_the_floating_bar() {
    let mut h = harness_for(DiagramKind::Erd);
    let a = add_shape(&mut h, "erd/table", Rect::new(100.0, 100.0, 380.0, 200.0));
    let b = add_shape(&mut h, "erd/table", Rect::new(600.0, 100.0, 880.0, 200.0));
    let primary_key = |h: &mut Ui, id| {
        app(h).doc.elements[&id]
            .as_shape()
            .unwrap()
            .erd
            .as_ref()
            .unwrap()
            .columns[0]
            .id
    };
    let source = primary_key(&mut h, a);
    let target = primary_key(&mut h, b);
    for (preset, label, marker) in [
        (ErdConnection::ExactlyOne, "Exactly one", Marker::ExactlyOne),
        (ErdConnection::ZeroOrOne, "Zero or one", Marker::ZeroOrOne),
        (ErdConnection::OneOrMany, "One or many", Marker::OneOrMany),
        (
            ErdConnection::ZeroOrMany,
            "Zero or many",
            Marker::ZeroOrMany,
        ),
        (ErdConnection::Many, "Many", Marker::Many),
    ] {
        app(&mut h).selection.clear();
        h.run();
        let before = app(&mut h).doc.clone();
        h.get_by_label(label).scroll_to_me();
        h.run();
        h.get_by_label(label).click();
        h.run();
        assert_eq!(app(&mut h).erd_connection, preset);
        assert_eq!(app(&mut h).tool, Tool::Connector);
        assert_eq!(
            app(&mut h).doc,
            before,
            "choosing a preset does not draw on the canvas"
        );
        let indicator_label = format!("ERD connection: {label}");
        let indicator = h.get_by_label(indicator_label.as_str());
        assert!(toolbar_rect(&h).contains_rect(indicator.rect()));
        let from = table_column_port(&mut h, a, source, false);
        let to = table_column_port(&mut h, b, target, true);
        let from = screen(&mut h, from);
        let to = screen(&mut h, to);
        drag(&mut h, from, to);
        let id = app(&mut h).selection[0];
        let connector = app(&mut h).doc.elements[&id]
            .as_connector()
            .expect("preset draws an ERD relationship");
        assert_eq!(
            (connector.start_marker, connector.end_marker),
            (Marker::ExactlyOne, marker),
            "selected cardinality survives table-row inference"
        );
        assert_eq!(connector.source.element(), Some(a));
        assert_eq!(connector.target.element(), Some(b));
        app(&mut h).undo();
        h.run();
        assert_eq!(app(&mut h).doc, before, "relationship is one undo step");
    }
}

#[test]
fn connection_shortcuts_cycle_erd_presets_without_changing_existing_lines() {
    let mut h = harness_for(DiagramKind::Erd);
    app(&mut h).insert_connector(
        Endpoint::Free(Point::new(100.0, 100.0)),
        Endpoint::Free(Point::new(300.0, 200.0)),
    );
    h.run();
    let before = app(&mut h).doc.clone();
    h.key_press(Key::C);
    h.run();
    assert_eq!(app(&mut h).tool, Tool::Connector);
    assert_eq!(app(&mut h).erd_connection, ErdConnection::ExactlyOne);
    for (preset, label) in [
        (ErdConnection::ZeroOrOne, "Zero or one"),
        (ErdConnection::OneOrMany, "One or many"),
        (ErdConnection::ZeroOrMany, "Zero or many"),
        (ErdConnection::Many, "Many"),
        (ErdConnection::ExactlyOne, "Exactly one"),
    ] {
        h.key_press_modifiers(Modifiers::SHIFT, Key::C);
        h.run();
        assert_eq!(app(&mut h).erd_connection, preset);
        assert_eq!(app(&mut h).tool, Tool::Connector);
        assert!(
            h.query_by_label(format!("ERD connection: {label}").as_str())
                .is_some()
        );
        assert_eq!(app(&mut h).doc, before);
    }
    h.key_press_modifiers(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::C);
    h.run();
    assert_eq!(app(&mut h).erd_connection, ErdConnection::ExactlyOne);
    assert_eq!(app(&mut h).doc, before);

    let erd = app(&mut h).page;
    app(&mut h).selection.clear();
    h.run();
    h.get_by_label("One or many").click();
    h.run();
    app(&mut h).add_page_with_kind(DiagramKind::Flowchart);
    h.run();
    app(&mut h).tool = Tool::Select;
    h.key_press_modifiers(Modifiers::SHIFT, Key::C);
    h.run();
    assert_eq!(app(&mut h).erd_connection, ErdConnection::ExactlyOne);
    assert!(h.query_by_label_contains("ERD connection:").is_none());
    h.key_press(Key::C);
    h.run();
    assert_eq!(
        app(&mut h).tool,
        Tool::Connector,
        "C keeps the flowchart connector shortcut"
    );
    app(&mut h).set_page(erd);
    h.run();
    assert_eq!(app(&mut h).erd_connection, ErdConnection::OneOrMany);
    assert!(h.query_by_label("ERD connection: One or many").is_some());
}

#[test]
fn connection_cycle_shortcut_does_not_interrupt_typing_or_quick_insert() {
    let mut h = harness_for(DiagramKind::Erd);
    let text = add_shape(&mut h, "basic/text", Rect::new(100.0, 100.0, 220.0, 132.0));
    app(&mut h).start_text_edit(text);
    h.run();
    let preset = app(&mut h).erd_connection;
    h.key_press_modifiers(Modifiers::SHIFT, Key::C);
    h.event(Event::Text("C".into()));
    h.run();
    assert_eq!(app(&mut h).erd_connection, preset);
    assert_eq!(app(&mut h).tool, Tool::Select);
    assert!(app(&mut h).editing.as_ref().unwrap().text.contains('C'));
    h.key_press_modifiers(Modifiers::COMMAND, Key::Enter);
    h.run();

    app(&mut h).open_quick_insert();
    h.run();
    h.key_press_modifiers(Modifiers::SHIFT, Key::C);
    h.event(Event::Text("C".into()));
    h.run();
    assert_eq!(app(&mut h).erd_connection, preset);
    assert_eq!(app(&mut h).tool, Tool::Select);
    assert_eq!(app(&mut h).quick_insert.as_ref().unwrap().query, "C");
}

#[test]
fn erd_connection_symbols_explain_cardinality_and_the_toolbar_can_choose_a_type() {
    let mut h = harness_for(DiagramKind::Erd);
    h.get_by_label("Many").scroll_to_me();
    h.run();
    h.get_by_label("Many").hover();
    h.run_steps(60);
    assert!(
        h.query_by_label_contains("multiple related records; no minimum is specified")
            .is_some(),
        "hovering a symbol explains its cardinality"
    );
    let before = app(&mut h).doc.clone();
    h.get_by_label("ERD connection: Exactly one").click();
    h.run();
    let canvas_left = app(&mut h).canvas_rect.left();
    h.get_all_by_label("Zero or one")
        .into_iter()
        .find(|node| node.rect().left() >= canvas_left)
        .expect("Zero or one is offered in the floating toolbar menu")
        .click();
    h.run();
    assert_eq!(app(&mut h).erd_connection, ErdConnection::ZeroOrOne);
    assert_eq!(app(&mut h).tool, Tool::Connector);
    assert_eq!(app(&mut h).doc, before);
    assert!(h.query_by_label("ERD connection: Zero or one").is_some());
}

#[test]
fn undoing_a_page_kind_change_cancels_tools_and_palette_drags_from_the_old_kind() {
    let mut h = harness_for(DiagramKind::Erd);
    app(&mut h).set_page_kind(DiagramKind::Flowchart);
    h.run();
    h.key_press(Key::D);
    h.run();
    let decision = ShapeRef::new("flowchart", "decision");
    assert_eq!(app(&mut h).tool, Tool::Shape(decision.clone()));
    app(&mut h).palette.dragging = Some(decision);
    app(&mut h).undo();
    assert_eq!(app(&mut h).page_kind(), Some(DiagramKind::Erd));
    assert_eq!(app(&mut h).tool, Tool::Select);
    assert!(app(&mut h).palette.dragging.is_none());
    h.run();
    h.key_press(Key::D);
    h.run();
    assert_eq!(
        app(&mut h).tool,
        Tool::Select,
        "the hidden Flowchart shortcut stays unavailable"
    );
    let point = screen(&mut h, Point::new(250.0, 250.0));
    click(&mut h, point);
    assert!(
        shapes(&mut h).is_empty(),
        "a stale tool cannot insert a hidden shape"
    );
}

#[test]
fn undoing_the_initial_page_choice_can_be_redone_through_the_chooser() {
    let mut h = harness_for(DiagramKind::Erd);
    let chosen = app(&mut h).doc.clone();
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!(app(&mut h).page_kind(), None);
    assert!(app(&mut h).page_choice.is_some());
    assert!(h.query_by_label("Choose diagram type").is_some());
    h.key_press_modifiers(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::Z);
    h.run();
    assert_eq!(app(&mut h).doc, chosen);
    assert_eq!(app(&mut h).page_kind(), Some(DiagramKind::Erd));
    assert!(app(&mut h).page_choice.is_none());
    assert!(h.query_by_label("Choose diagram type").is_none());
    assert!(h.query_by_label("Table").is_some());

    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    h.get_by_label("Restore previous page").click();
    h.run();
    assert_eq!(app(&mut h).doc, chosen);
    assert!(app(&mut h).page_choice.is_none());
    assert!(h.query_by_label("Choose diagram type").is_none());
}

#[test]
fn new_and_open_clear_palette_search_and_legacy_page_inference_preserves_content() {
    let mut h = harness_for(DiagramKind::Erd);
    add_shape(&mut h, "erd/table", Rect::new(100.0, 100.0, 380.0, 200.0));
    let mut legacy = app(&mut h).doc.clone();
    let page = app(&mut h).page;
    legacy.pages.get_mut(&page).unwrap().diagram_kind = None;
    let path = std::env::temp_dir().join(format!(
        "blueprint-ui-legacy-{}.blueprint.json",
        ElementId::new()
    ));
    bp_io::save(&legacy, &path).unwrap();

    app(&mut h).palette.query = "outdated search".into();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::N);
    h.run();
    h.get_by_label("Don't save").click();
    h.run();
    assert!(app(&mut h).palette.query.is_empty());
    assert!(app(&mut h).doc.elements.is_empty());
    assert_eq!(app(&mut h).page_kind(), None);
    h.get_by_label("Flowchart").click();
    h.run();
    app(&mut h).palette.query = "another outdated search".into();
    app(&mut h).open_path(&path);
    h.run();
    std::fs::remove_file(path).unwrap();
    assert!(app(&mut h).palette.query.is_empty());
    assert_eq!(app(&mut h).page_kind(), Some(DiagramKind::Erd));
    assert!(app(&mut h).page_choice.is_none());
    assert_eq!(
        app(&mut h).doc,
        legacy,
        "inferring a legacy page type leaves the document intact"
    );
    assert!(!app(&mut h).is_dirty());
    assert!(h.query_all_by_label("Table").next().is_some());
    assert!(h.query_by_label("Choose diagram type").is_none());
}
