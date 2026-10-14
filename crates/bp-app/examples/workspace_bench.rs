//! Diagnostic CPU profiling of the complete application UI, without a GPU.
//! `cargo run --release -p bp-app --example workspace_bench -- [sample_frames]`
//!
//! The app/egui column includes real input, app.frame, scene refresh, all docks,
//! text layout and paint-list generation. CPU total also includes tessellation.
//! Native windowing, GPU uploads/presentation and cold startup are excluded.
//! This reports measurements; it deliberately sets no CI timing thresholds.

use bp_app::{BlueprintApp, LeftTab, WorkspacePreferences};
use bp_commands::Command;
use bp_icons::IconPack;
use bp_model::kurbo::Rect;
use bp_model::{
    CloudIcon, CloudProvider, Color, DiagramKind, Element, ElementId, Endpoint, IconKind, OrderKey,
    Paint, Parent, ShapeRef,
};
use egui::{Event, Modifiers, PointerButton, Pos2, RawInput, Vec2, ViewportId};
use std::sync::Arc;
use std::time::{Duration, Instant};

const SHAPE_COUNT: usize = 2500;
const ICON_COUNT: usize = 5000;
const WARMUP_FRAMES: usize = 20;
const WINDOW: Vec2 = Vec2::new(1360.0, 860.0);

#[derive(Clone, Copy)]
struct Timing {
    ui: Duration,
    total: Duration,
}

struct Driver {
    ctx: egui::Context,
    app: BlueprintApp,
    frame_number: u64,
}

impl Driver {
    fn new(preferences: WorkspacePreferences, catalogs: Vec<IconPack>, kind: DiagramKind) -> Self {
        let ctx = egui::Context::default();
        let mut app = BlueprintApp::new_with_workspace(&ctx, None, preferences, catalogs);
        app.choose_page_kind(kind);
        Self {
            ctx,
            app,
            frame_number: 0,
        }
    }

    fn frame(&mut self, events: Vec<Event>) -> Timing {
        let mut input = RawInput {
            screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, WINDOW)),
            time: Some(self.frame_number as f64 / 60.0),
            predicted_dt: 1.0 / 60.0,
            events,
            ..Default::default()
        };
        input
            .viewports
            .get_mut(&ViewportId::ROOT)
            .expect("root viewport")
            .native_pixels_per_point = Some(1.0);
        self.frame_number += 1;
        let started = Instant::now();
        let mut output = self.ctx.run_ui(input, |ui| self.app.frame(ui));
        let ui = started.elapsed();
        let shapes = std::mem::take(&mut output.shapes);
        let meshes = self.ctx.tessellate(shapes, output.pixels_per_point);
        std::hint::black_box(meshes);
        let total = started.elapsed();
        output.drop_without_applying_deltas();
        Timing { ui, total }
    }

    fn warmup(&mut self) {
        for _ in 0..WARMUP_FRAMES {
            self.frame(Vec::new());
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let frames = match args.next().as_deref() {
        Some("--help" | "-h") => {
            println!(
                "Usage: workspace_bench [sample_frames]\nDefault: 120 frames per case. Run with --release for representative timings."
            );
            return Ok(());
        }
        Some(value) => value.parse::<usize>()?,
        None => 120,
    };
    if frames == 0 || args.next().is_some() {
        return Err("Use one positive sample_frames value; see --help".into());
    }
    println!(
        "Blueprint complete CPU UI profile: {} build, {}x{} logical points, scale 1, {frames} samples per case",
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        WINDOW.x,
        WINDOW.y,
    );
    println!("app/egui = run_ui + actual app.frame; CPU total adds mesh tessellation.");
    println!("Excludes cold startup, GPU work and native event-loop/presentation costs.");
    println!(
        "Synthetic cloud SVGs are original, simple vector artwork, not a vendor-pack complexity claim."
    );
    println!(
        "{:<30} {:>21} {:>21}",
        "Case", "app/egui median / p95", "CPU total median / p95"
    );

    let mut scene = Driver::new(
        WorkspacePreferences {
            left_tab: LeftTab::Navigator,
            ..Default::default()
        },
        Vec::new(),
        DiagramKind::Flowchart,
    );
    let ids = populate_scene(&mut scene.app);
    scene.warmup();
    assert_eq!(scene.app.scene.order.len(), 5000);
    let document = scene.app.doc.clone();
    measure(&mut scene, "Scene/Navigator: warm", frames, |_, _| {
        Vec::new()
    });
    measure(
        &mut scene,
        "Scene/Navigator: wheel pan",
        frames,
        |driver, _| wheel(driver.app.canvas_rect.center(), Vec2::new(-4.0, 0.0)),
    );
    scene.app.view.pan = Vec2::new(40.0, 40.0);
    scene.warmup();
    measure(
        &mut scene,
        "Scene/Navigator: selection",
        frames,
        |driver, index| {
            // Alternate distant visible shapes to exercise actual pointer selection
            // without interpreting consecutive clicks as in-place text editing.
            let id = ids[index % 2];
            let bounds = driver.app.scene.shape(id).expect("benchmark shape").bounds;
            let point = driver
                .app
                .view
                .to_screen(driver.app.canvas_rect.min, bounds.center());
            assert!(driver.app.canvas_rect.contains(point));
            vec![
                Event::PointerMoved(point),
                Event::PointerButton {
                    pos: point,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                },
                Event::PointerButton {
                    pos: point,
                    button: PointerButton::Primary,
                    pressed: false,
                    modifiers: Modifiers::NONE,
                },
            ]
        },
    );
    assert!(!scene.app.selection.is_empty());
    assert!(scene.app.editing.is_none());
    assert_eq!(
        scene.app.doc, document,
        "pan/selection must not modify the fixture"
    );
    scene.app.selection.clear();
    scene.app.fit_requested = true;
    scene.warmup();
    measure(
        &mut scene,
        "Scene/Navigator: fit-all warm",
        frames,
        |_, _| Vec::new(),
    );

    let mut catalog = Driver::new(
        WorkspacePreferences::default(),
        vec![synthetic_catalog()],
        DiagramKind::Cloud,
    );
    catalog.warmup();
    assert_eq!(catalog.app.cloud.packs[0].icons.len(), ICON_COUNT);
    measure(&mut catalog, "Cloud 5,000 icons: warm", frames, |_, _| {
        Vec::new()
    });
    catalog.app.preferences.favorites = catalog.app.cloud.packs[0]
        .icons
        .iter()
        .take(1000)
        .map(|icon| icon.reference.clone())
        .collect();
    catalog.warmup();
    measure(&mut catalog, "Cloud: 1,000 favorites", frames, |_, _| {
        Vec::new()
    });
    catalog.app.preferences.favorites.clear();
    measure(
        &mut catalog,
        "Cloud 5,000 icons: filter",
        frames,
        |driver, index| {
            // Input preparation is untimed; search/cache recomputation and the
            // changed result widgets run inside the measured application frame.
            driver.app.palette.query = format!("Service {:05}", index * 37 % ICON_COUNT);
            Vec::new()
        },
    );
    println!("All timings are milliseconds; each case reports its own measured distribution.");
    Ok(())
}

fn measure(
    driver: &mut Driver,
    label: &str,
    frames: usize,
    mut input: impl FnMut(&mut Driver, usize) -> Vec<Event>,
) {
    let timings: Vec<_> = (0..frames)
        .map(|index| {
            let events = input(driver, index);
            driver.frame(events)
        })
        .collect();
    let (ui_median, ui_p95) = distribution(timings.iter().map(|timing| timing.ui));
    let (total_median, total_p95) = distribution(timings.iter().map(|timing| timing.total));
    println!("{label:<30} {ui_median:>9.3} / {ui_p95:>9.3} {total_median:>9.3} / {total_p95:>9.3}");
}

fn distribution(values: impl Iterator<Item = Duration>) -> (f64, f64) {
    let mut values: Vec<_> = values.map(|value| value.as_secs_f64() * 1000.0).collect();
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    let median = if values.len().is_multiple_of(2) {
        (values[middle - 1] + values[middle]) / 2.0
    } else {
        values[middle]
    };
    let p95 = values[(values.len() * 95).div_ceil(100) - 1];
    (median, p95)
}

fn wheel(at: Pos2, delta: Vec2) -> Vec<Event> {
    vec![
        Event::PointerMoved(at),
        Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta,
            phase: egui::TouchPhase::Move,
            modifiers: Modifiers::NONE,
        },
    ]
}

fn populate_scene(app: &mut BlueprintApp) -> Vec<ElementId> {
    let parent = Parent::Layer(app.layer);
    let mut order = OrderKey::first();
    let mut ids = Vec::with_capacity(SHAPE_COUNT);
    let mut commands = Vec::with_capacity(SHAPE_COUNT * 2);
    for index in 0..SHAPE_COUNT {
        let x = (index % 50) as f64 * 180.0 + 40.0;
        let y = (index / 50) as f64 * 120.0 + 40.0;
        let mut element = Element::shape(
            ShapeRef::new("flowchart", "process"),
            parent,
            order.clone(),
            Rect::new(x, y, x + 130.0, y + 60.0),
        );
        order = OrderKey::after(&order);
        let shape = element.as_shape_mut().expect("created shape");
        shape.text = format!("Validate order {index:04}");
        shape.style.fill = Some(Paint::Color(Color::rgb(0x27, 0x35, 0x48)));
        shape.style.stroke = Some(Paint::Color(Color::rgb(0xb8, 0xc4, 0xd3)));
        shape.style.text_color = Some(Color::rgb(0xee, 0xf2, 0xf6));
        ids.push(element.id);
        commands.push(Command::Insert(Box::new(element)));
    }
    for index in 0..SHAPE_COUNT {
        let target = if index % 50 < 49 {
            index + 1
        } else if index + 50 < SHAPE_COUNT {
            index + 50
        } else {
            index - 1
        };
        let mut element = Element::connector(
            Endpoint::glued(ids[index], None),
            Endpoint::glued(ids[target], None),
            parent,
            order.clone(),
        );
        order = OrderKey::after(&order);
        let connector = element.as_connector_mut().expect("created connector");
        connector.style.stroke = Some(Paint::Color(Color::rgb(0xb8, 0xc4, 0xd3)));
        commands.push(Command::Insert(Box::new(element)));
    }
    assert!(app.apply("Build CPU profile fixture", commands));
    app.selection.clear();
    app.doc.validate().expect("valid 5,000-element scene");
    ids
}

fn synthetic_catalog() -> IconPack {
    let icons = (0..ICON_COUNT)
        .map(|index| {
            let color = (index as u32).wrapping_mul(0x45d9f3b) & 0x00ff_ffff;
            CloudIcon {
                reference: ShapeRef::new("aws", &format!("original-service-{index:05}@bench-v1")),
                name: format!("Service {index:05}"),
                provider: CloudProvider::Aws,
                category: ["Compute", "Networking", "Database", "Storage"][index % 4].into(),
                kind: IconKind::Service,
                pack_version: "bench-v1".into(),
                source_path: format!("Original profiling artwork/{index:05}.svg"),
                svg: Arc::from(format!(
                    "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"64\" height=\"64\"><rect x=\"4\" y=\"4\" width=\"56\" height=\"56\" rx=\"8\" fill=\"#{color:06x}\"/><path d=\"M20 20h24v24H20zM20 32h24M32 20v24\" fill=\"none\" stroke=\"#ffffff\" stroke-width=\"2\"/></svg>"
                )),
            }
        })
        .collect();
    IconPack {
        provider: CloudProvider::Aws,
        version: "bench-v1".into(),
        warnings: Vec::new(),
        icons,
    }
}
