//! Renders the real app to a PNG, for checking the UI by eye:
//! `cargo run -p bp-app --example screenshot -- out.png [diagram.blueprint | erd | flowchart | cloud] [mode]`
//!
//! Modes: `shape` (select the first shape), `connector` (the first
//! connector), `all` (everything), `drag` (mid-way through moving the
//! first shape, showing guides), `connect` (mid-way through drawing a
//! connector from the first shape's east port). Without a mode nothing is
//! selected.

use bp_app::BlueprintApp;
use bp_model::DiagramKind;
use bp_model::kurbo::{Point, Vec2};
use egui::{Event, Modifiers, PointerButton, Pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use std::path::PathBuf;

fn main() {
    let mut args = std::env::args().skip(1);
    let out = args.next().unwrap_or_else(|| "blueprint.png".into());
    let source = args.next();
    let kind = match source.as_deref() {
        Some("erd") => Some(DiagramKind::Erd),
        Some("flowchart") => Some(DiagramKind::Flowchart),
        Some("cloud") => Some(DiagramKind::Cloud),
        _ => None,
    };
    let file = source.filter(|_| kind.is_none()).map(PathBuf::from);
    let mode = args.next().unwrap_or_default();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1360.0, 860.0))
        .with_step_dt(1.0 / 60.0)
        .wgpu()
        .build_ui_state(
            move |ui, app: &mut Option<BlueprintApp>| {
                let app = app.get_or_insert_with(|| BlueprintApp::new(ui.ctx(), file.clone()));
                if let Some(kind) = kind {
                    app.choose_page_kind(kind);
                }
                app.frame(ui);
            },
            None,
        );
    h.run();

    let app = h.state_mut().as_mut().unwrap();
    let page = app.page;
    let order: Vec<_> = app
        .doc
        .paint_order(page)
        .iter()
        .map(|e| (e.id, e.is_shape(), e.is_connector()))
        .collect();
    let first_shape = order.iter().find(|e| e.1).map(|e| e.0);
    let first_connector = order.iter().find(|e| e.2).map(|e| e.0);
    let to_screen = |app: &BlueprintApp, p: Point| app.view.to_screen(app.canvas_rect.min, p);
    let press_and_move = |h: &mut Harness<'_, Option<BlueprintApp>>, from: Pos2, to: Pos2| {
        h.hover_at(from);
        h.event(Event::PointerButton {
            pos: from,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        });
        for i in 1..=10 {
            h.hover_at(from + (to - from) * (i as f32 / 10.0));
        }
        h.step();
    };
    match mode.as_str() {
        "shape" => app.selection = first_shape.into_iter().collect(),
        "connector" => app.selection = first_connector.into_iter().collect(),
        "all" => app.select_all(),
        "packs" => app.open_cloud_manager(),
        "calendar" => {
            app.cloud.version = "2026-07-31".into();
            app.open_cloud_manager();
        }
        "drag" => {
            if let Some(id) = first_shape {
                let b = app.doc.elements[&id].as_shape().unwrap().bounds;
                let from = to_screen(app, b.center());
                let to = to_screen(app, b.center() + Vec2::new(183.0, 2.0));
                press_and_move(&mut h, from, to);
            }
        }
        "connect" => {
            if let Some(id) = first_shape {
                app.refresh_scene();
                let g = app.scene.shape(id).unwrap();
                let east = g.ports.iter().find(|p| p.id.as_str() == "e").unwrap().at;
                let from = to_screen(app, east);
                let to = to_screen(app, east + Vec2::new(160.0, 120.0));
                h.hover_at(from);
                h.step();
                press_and_move(&mut h, from, to);
            }
        }
        _ => {}
    }
    h.run_steps(2);
    if mode == "calendar" {
        h.get_by_label("Choose pack release date").click();
        h.run_steps(2);
    }
    let image = h.render().expect("render");
    image.save(&out).expect("save png");
    println!("Wrote {out}");
}
