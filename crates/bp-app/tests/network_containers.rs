//! Real egui gestures exercise containment, history, cancellation and boundary resizing.
use bp_app::{BlueprintApp, WorkspacePreferences};
use bp_commands::{Command, Prop};
use bp_model::kurbo::{Point, Rect, Vec2};
use bp_model::{
    AwsResource, DiagramKind, Element, ElementId, NetworkContainer, OrderKey, Parent, ShapeRef,
};
use egui::{Event, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::Harness;

type Ui = Harness<'static, Option<(BlueprintApp, Ids)>>;
#[derive(Clone, Copy)]
struct Ids {
    vpc: ElementId,
    a: ElementId,
    b: ElementId,
    service: ElementId,
}
fn app(h: &mut Ui) -> &mut BlueprintApp {
    &mut h.state_mut().as_mut().unwrap().0
}
fn ids(h: &Ui) -> Ids {
    h.state().as_ref().unwrap().1
}
fn fixture() -> Ui {
    let mut h = Harness::builder()
        .with_size(egui::vec2(1600.0, 960.0))
        .with_step_dt(1.0 / 60.0)
        .build_ui_state(
            |ui, state: &mut Option<(BlueprintApp, Ids)>| {
                if state.is_none() {
                    let mut app = BlueprintApp::new_with_workspace(
                        ui.ctx(),
                        None,
                        WorkspacePreferences::default(),
                        Vec::new(),
                    );
                    app.choose_page_kind(DiagramKind::Cloud);
                    app.settings.snap_to_grid = false;
                    app.settings.snap_to_shapes = false;
                    let parent = Parent::Layer(app.layer);
                    let vpc = container(
                        &mut app,
                        NetworkContainer::Vpc,
                        parent,
                        Rect::new(0.0, 0.0, 1000.0, 560.0),
                        AwsResource::Vpc {
                            cidr: "10.0.0.0/16".into(),
                        },
                    );
                    let a = container(
                        &mut app,
                        NetworkContainer::Subnet,
                        Parent::Element(vpc),
                        Rect::new(30.0, 90.0, 470.0, 510.0),
                        AwsResource::Subnet {
                            vpc: Some(vpc),
                            cidr: "10.0.1.0/24".into(),
                            availability_zone: "us-east-1a".into(),
                        },
                    );
                    let b = container(
                        &mut app,
                        NetworkContainer::Subnet,
                        Parent::Element(vpc),
                        Rect::new(510.0, 90.0, 950.0, 510.0),
                        AwsResource::Subnet {
                            vpc: Some(vpc),
                            cidr: "10.0.2.0/24".into(),
                            availability_zone: "us-east-1b".into(),
                        },
                    );
                    let service = app
                        .insert_shape(
                            ShapeRef::new("basic", "rectangle"),
                            Rect::new(110.0, 210.0, 174.0, 274.0),
                        )
                        .unwrap();
                    app.apply(
                        "Place service",
                        bp_commands::network::drop(&app.doc, &[service], Some(a), Vec2::ZERO)
                            .unwrap(),
                    );
                    // Include a visible open-canvas destination in the fit area.
                    app.insert_shape(
                        ShapeRef::new("basic", "text"),
                        Rect::new(1080.0, 300.0, 1240.0, 370.0),
                    );
                    app.selection.clear();
                    app.fit_requested = true;
                    *state = Some((app, Ids { vpc, a, b, service }));
                }
                state.as_mut().unwrap().0.frame(ui);
            },
            None,
        );
    h.run();
    h
}
fn container(
    app: &mut BlueprintApp,
    kind: NetworkContainer,
    parent: Parent,
    bounds: Rect,
    resource: AwsResource,
) -> ElementId {
    let mut element = Element::shape(
        ShapeRef::new("basic", "rectangle"),
        parent,
        OrderKey::first(),
        bounds,
    );
    let shape = element.as_shape_mut().unwrap();
    shape.network_container = Some(kind);
    shape.infrastructure = Some(resource);
    shape.text = kind.label().into();
    let id = element.id;
    assert!(app.apply("Fixture", [Command::Insert(Box::new(element))]));
    id
}
fn screen(h: &mut Ui, p: Point) -> Pos2 {
    let app = app(h);
    app.view.to_screen(app.canvas_rect.min, p)
}
fn bounds(h: &mut Ui, id: ElementId) -> Rect {
    app(h).doc.elements[&id].as_shape().unwrap().bounds
}
fn assert_rect(actual: Rect, expected: Rect) {
    for (actual, expected) in [
        (actual.x0, expected.x0),
        (actual.y0, expected.y0),
        (actual.x1, expected.x1),
        (actual.y1, expected.y1),
    ] {
        assert!(
            (actual - expected).abs() < 0.01,
            "{actual} differs from {expected}"
        );
    }
}
fn press(h: &mut Ui, from: Point, to: Point) -> Pos2 {
    let from = screen(h, from);
    let to = screen(h, to);
    h.hover_at(from);
    h.drag_at(from);
    for step in 1..=8 {
        h.hover_at(from + (to - from) * (step as f32 / 8.0));
    }
    h.step();
    to
}
fn release(h: &mut Ui, at: Pos2) {
    h.event(Event::PointerButton {
        pos: at,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(4);
}
fn drag(h: &mut Ui, from: Point, to: Point) {
    let at = press(h, from, to);
    release(h, at);
}

#[test]
fn service_transfer_detach_cancel_and_history_use_complete_gestures() {
    let mut h = fixture();
    let ids = ids(&h);
    assert_eq!(app(&mut h).selectable(ids.service), ids.service);
    let before = app(&mut h).doc.clone();
    drag(&mut h, Point::new(142.0, 242.0), Point::new(660.0, 300.0));
    assert_eq!(app(&mut h).selection, vec![ids.service]);
    assert_eq!(
        app(&mut h).doc.elements[&ids.service].parent,
        Parent::Element(ids.b)
    );
    let transferred = app(&mut h).doc.clone();
    app(&mut h).undo();
    assert_eq!(app(&mut h).doc, before);
    app(&mut h).redo();
    assert_eq!(app(&mut h).doc, transferred);
    h.run_steps(30);
    let at = press(&mut h, Point::new(660.0, 300.0), Point::new(1120.0, 440.0));
    h.key_press(Key::Escape);
    h.run_steps(2);
    release(&mut h, at);
    assert_eq!(app(&mut h).doc, transferred);
    h.run_steps(30);
    drag(&mut h, Point::new(660.0, 300.0), Point::new(1120.0, 440.0));
    let layer = app(&mut h).layer;
    assert_eq!(
        app(&mut h).doc.elements[&ids.service].parent,
        Parent::Layer(layer)
    );
    app(&mut h).undo();
    assert_eq!(app(&mut h).doc, transferred);
}

#[test]
fn headers_move_descendants_once_and_resize_only_the_boundary() {
    let mut h = fixture();
    let ids = ids(&h);
    let service = bounds(&mut h, ids.service);
    let subnet = bounds(&mut h, ids.a);
    app(&mut h).selection = vec![ids.vpc, ids.a, ids.service];
    drag(&mut h, Point::new(220.0, 25.0), Point::new(240.0, 45.0));
    assert_rect(bounds(&mut h, ids.service), service + Vec2::new(20.0, 20.0));
    assert_rect(bounds(&mut h, ids.a), subnet + Vec2::new(20.0, 20.0));
    let contents = bounds(&mut h, ids.service);
    let nested = bounds(&mut h, ids.a);
    app(&mut h).select_only(ids.vpc);
    h.run_steps(30);
    let vpc = bounds(&mut h, ids.vpc);
    drag(
        &mut h,
        Point::new(vpc.x1, vpc.center().y),
        Point::new(vpc.x1 + 70.0, vpc.center().y),
    );
    assert!((bounds(&mut h, ids.vpc).width() - vpc.width() - 70.0).abs() < 0.01);
    assert_eq!(bounds(&mut h, ids.service), contents);
    assert_eq!(bounds(&mut h, ids.a), nested);
    app(&mut h).undo();
    assert_eq!(bounds(&mut h, ids.vpc), vpc);
}

#[test]
fn locked_descendants_block_indirect_moves_and_bad_drops_roll_back() {
    let mut h = fixture();
    let ids = ids(&h);
    app(&mut h).apply(
        "Lock service",
        [Command::Set {
            id: ids.service,
            prop: Prop::Locked(true),
        }],
    );
    let before = app(&mut h).doc.clone();
    drag(&mut h, Point::new(220.0, 25.0), Point::new(240.0, 45.0));
    assert_eq!(app(&mut h).doc, before);
    app(&mut h).apply(
        "Unlock service",
        [Command::Set {
            id: ids.service,
            prop: Prop::Locked(false),
        }],
    );
    app(&mut h).apply(
        "Lock destination",
        [Command::Set {
            id: ids.b,
            prop: Prop::Locked(true),
        }],
    );
    h.run_steps(30);
    let before = app(&mut h).doc.clone();
    drag(&mut h, Point::new(142.0, 242.0), Point::new(660.0, 300.0));
    assert_eq!(app(&mut h).doc, before);
    assert!(app(&mut h).status.to_lowercase().contains("locked"));
    h.run_steps(30);
    drag(&mut h, Point::new(230.0, 115.0), Point::new(1120.0, 440.0));
    assert_eq!(app(&mut h).doc, before);
    assert!(
        app(&mut h)
            .status
            .to_lowercase()
            .contains("assigned subnet")
    );
}

#[test]
fn subnet_transfer_updates_reference_and_undo_restores_services() {
    let mut h = fixture();
    let ids = ids(&h);
    let parent = Parent::Layer(app(&mut h).layer);
    let dest = container(
        app(&mut h),
        NetworkContainer::Vpc,
        parent,
        Rect::new(1100.0, 0.0, 2100.0, 560.0),
        AwsResource::Vpc {
            cidr: "10.0.0.0/16".into(),
        },
    );
    app(&mut h).selection.clear();
    app(&mut h).fit_requested = true;
    h.run_steps(4);
    let before = app(&mut h).doc.clone();
    drag(&mut h, Point::new(220.0, 115.0), Point::new(1320.0, 115.0));
    let status = app(&mut h).status.clone();
    assert_eq!(
        app(&mut h).doc.elements[&ids.a].parent,
        Parent::Element(dest),
        "{}",
        status
    );
    assert!(
        matches!(app(&mut h).doc.elements[&ids.a].as_shape().unwrap().infrastructure,Some(AwsResource::Subnet{vpc:Some(id),..}) if id==dest)
    );
    let after = app(&mut h).doc.clone();
    app(&mut h).undo();
    assert_eq!(app(&mut h).doc, before);
    app(&mut h).redo();
    assert_eq!(app(&mut h).doc, after);
    assert_rect(
        bounds(&mut h, ids.service),
        before.elements[&ids.service].as_shape().unwrap().bounds + Vec2::new(1100.0, 0.0),
    );
}

#[test]
fn empty_container_interiors_allow_service_marquee_selection() {
    let mut h = fixture();
    let ids = ids(&h);
    drag(&mut h, Point::new(80.0, 180.0), Point::new(200.0, 310.0));
    assert_eq!(app(&mut h).selection, vec![ids.service]);
    assert_eq!(app(&mut h).scope, None);
}

#[test]
fn palette_drop_attaches_as_one_step_and_escape_cancels_insertion() {
    let mut h = fixture();
    let ids = ids(&h);
    let before = app(&mut h).doc.clone();
    let source = app(&mut h).workspace.left_rect.left_top() + egui::vec2(5.0, 5.0);
    h.hover_at(source);
    h.drag_at(source);
    h.step();
    app(&mut h).palette.dragging = Some(ShapeRef::new("basic", "rectangle"));
    let destination = screen(&mut h, Point::new(720.0, 360.0));
    h.hover_at(destination);
    h.step();
    release(&mut h, destination);
    let inserted = *app(&mut h).selection.first().unwrap();
    assert_eq!(app(&mut h).doc.elements.len(), before.elements.len() + 1);
    assert_eq!(
        app(&mut h).doc.elements[&inserted].parent,
        Parent::Element(ids.b)
    );
    app(&mut h).undo();
    assert_eq!(app(&mut h).doc, before);
    h.run_steps(30);
    h.hover_at(source);
    h.drag_at(source);
    h.step();
    app(&mut h).palette.dragging = Some(ShapeRef::new("basic", "rectangle"));
    h.hover_at(destination);
    h.key_press(Key::Escape);
    release(&mut h, destination);
    assert_eq!(app(&mut h).doc, before);
}

#[test]
fn editing_a_service_group_preserves_it_within_the_current_subnet() {
    let mut h = fixture();
    let ids = ids(&h);
    let parent = Parent::Element(ids.a);
    let group = Element::group(parent, app(&mut h).doc.next_order_key(parent));
    let group_id = group.id;
    app(&mut h).apply(
        "Group service",
        [
            Command::Insert(Box::new(group)),
            Command::Set {
                id: ids.service,
                prop: Prop::Parent(Parent::Element(group_id)),
            },
        ],
    );
    app(&mut h).scope = Some(group_id);
    app(&mut h).select_only(ids.service);
    h.run_steps(3);
    let before = app(&mut h).doc.clone();
    drag(&mut h, Point::new(142.0, 242.0), Point::new(230.0, 280.0));
    assert_eq!(
        app(&mut h).doc.elements[&ids.service].parent,
        Parent::Element(group_id)
    );
    let after = app(&mut h).doc.clone();
    app(&mut h).undo();
    assert_eq!(app(&mut h).doc, before);
    app(&mut h).redo();
    assert_eq!(app(&mut h).doc, after);
    h.run_steps(30);
    drag(&mut h, Point::new(230.0, 280.0), Point::new(660.0, 300.0));
    assert_eq!(
        app(&mut h).doc.elements[&ids.service].parent,
        Parent::Element(ids.b)
    );
}

#[test]
fn pasting_a_network_into_a_subnet_group_rejects_without_partial_copies() {
    let mut h = fixture();
    let ids = ids(&h);
    app(&mut h).select_only(ids.vpc);
    let ctx = h.ctx.clone();
    app(&mut h).copy(&ctx);
    let clip = app(&mut h).clip.clone().unwrap();
    let parent = Parent::Element(ids.a);
    let group = Element::group(parent, app(&mut h).doc.next_order_key(parent));
    let group_id = group.id;
    app(&mut h).apply("Service group", [Command::Insert(Box::new(group))]);
    app(&mut h).scope = Some(group_id);
    let before = app(&mut h).doc.clone();
    app(&mut h).paste_text(&clip);
    assert_eq!(app(&mut h).doc, before);
    assert!(app(&mut h).status.starts_with("Cannot paste here:"));
    assert!(app(&mut h).doc.validate().is_ok());
}

#[test]
fn palette_insertions_respect_group_membership_and_open_canvas_detachment() {
    let mut h = fixture();
    let ids = ids(&h);
    let parent = Parent::Element(ids.a);
    let group = Element::group(parent, app(&mut h).doc.next_order_key(parent));
    let group_id = group.id;
    app(&mut h).apply("Service group", [Command::Insert(Box::new(group))]);
    app(&mut h).scope = Some(group_id);
    let before = app(&mut h).doc.clone();
    let source = app(&mut h).workspace.left_rect.left_top() + egui::vec2(5.0, 5.0);
    for (point, expected_parent) in [
        (Point::new(300.0, 360.0), Parent::Element(group_id)),
        (Point::new(1120.0, 440.0), Parent::Layer(app(&mut h).layer)),
    ] {
        h.hover_at(source);
        h.drag_at(source);
        h.step();
        app(&mut h).palette.dragging = Some(ShapeRef::new("basic", "rectangle"));
        let to = screen(&mut h, point);
        h.hover_at(to);
        h.step();
        release(&mut h, to);
        let inserted = *app(&mut h)
            .selection
            .first()
            .expect("palette insertion succeeds");
        assert_eq!(app(&mut h).doc.elements[&inserted].parent, expected_parent);
        app(&mut h).undo();
        assert_eq!(app(&mut h).doc, before);
        h.run_steps(30);
    }
}
