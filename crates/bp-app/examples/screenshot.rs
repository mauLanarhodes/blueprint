//! Render the real app to a PNG for visual review.
//!
//! Existing positional usage is retained:
//! `cargo run -p bp-app --example screenshot -- out.png [diagram.blueprint | erd | flowchart | cloud] [mode]`
//!
//! For populated, isolated examples:
//! `cargo run -p bp-app --example screenshot -- out.png --fixture erd --mode connector --fit --size 1360x860 --scale 1.25`

#[path = "support/fixtures.rs"]
mod fixtures;

use bp_app::{BlueprintApp, LeftTab, WorkspacePreferences, WorkspaceTheme};
use bp_model::DiagramKind;
use bp_model::kurbo::Vec2;
use egui::{Event, Modifiers, PointerButton, Pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use std::path::PathBuf;

const HELP: &str = "Render Blueprint's real UI to a PNG.

Usage: screenshot [out.png] [diagram.blueprint | erd | flowchart | cloud] [mode] [options]
       screenshot [out.png] --fixture erd|flowchart|cloud [mode] [options]

Options:
  --fixture KIND      Build a populated example with original fixture artwork
  --mode MODE         none, shape, column, connector, focus, all, drag, connect, packs,
                      network, network-services, network-drop, network-transfer,
                      deploy, calendar, sql-warning, long-text, or error
                      (also accepted positionally)
  --size WIDTHxHEIGHT  Window size in logical points (default: 1360x860)
  --scale NUMBER       Pixels per logical point, greater than 0 through 4 (default: 1)
  --navigator          Show the actual page/layer/element navigator
  --preferences FILE   Load only this workspace-preferences JSON file
  --catalogs DIRECTORY Load only these locally installed icon packs
  --svg FILE           Export the final page to SVG for geometry comparison
  --fit                Fit the final canvas layout (before drag/connect gestures)
  --theme dark|light   Workspace theme (default: dark)
  --help               Print this help

User workspace preferences and cloud catalogs are loaded only with explicit inputs.
The deploy mode opens the existing dialog; it never runs Terraform or AWS.";

#[derive(Clone)]
struct Options {
    output: PathBuf,
    file: Option<PathBuf>,
    kind: Option<DiagramKind>,
    fixture: Option<DiagramKind>,
    mode: String,
    size: egui::Vec2,
    scale: f32,
    fit: bool,
    navigator: bool,
    theme: Option<WorkspaceTheme>,
    preferences: Option<PathBuf>,
    catalogs: Option<PathBuf>,
    svg: Option<PathBuf>,
}

impl Options {
    fn parse() -> Result<Option<Self>, String> {
        let mut args = std::env::args().skip(1);
        let mut positional = Vec::new();
        let mut fixture = None;
        let mut requested_mode = None;
        let mut size = egui::vec2(1360.0, 860.0);
        let mut scale = 1.0;
        let mut fit = false;
        let mut navigator = false;
        let mut theme = None;
        let mut preferences = None;
        let mut catalogs = None;
        let mut svg = None;
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--help" | "-h" => return Ok(None),
                "--fixture" => fixture = Some(diagram_kind(&next(&mut args, "--fixture")?)?),
                "--mode" => requested_mode = Some(next(&mut args, "--mode")?),
                "--size" => {
                    let value = next(&mut args, "--size")?;
                    let (width, height) = value
                        .split_once(['x', 'X'])
                        .ok_or("--size needs WIDTHxHEIGHT, for example 1360x860")?;
                    let dimension = |value: &str| -> Result<f32, String> {
                        let number = value
                            .parse::<u32>()
                            .map_err(|_| "--size needs positive integer dimensions".to_owned())?;
                        if number == 0 {
                            return Err("--size dimensions must be greater than zero".into());
                        }
                        Ok(number as f32)
                    };
                    size = egui::vec2(dimension(width)?, dimension(height)?);
                }
                "--scale" => {
                    scale = next(&mut args, "--scale")?
                        .parse::<f32>()
                        .map_err(|_| "--scale needs a number, for example 1.25".to_owned())?;
                    if !scale.is_finite() || scale <= 0.0 || scale > 4.0 {
                        return Err("--scale must be greater than zero and at most 4".into());
                    }
                }
                "--fit" => fit = true,
                "--navigator" => navigator = true,
                "--preferences" => preferences = Some(next(&mut args, "--preferences")?.into()),
                "--catalogs" => catalogs = Some(next(&mut args, "--catalogs")?.into()),
                "--svg" => svg = Some(next(&mut args, "--svg")?.into()),
                "--theme" => {
                    theme = Some(match next(&mut args, "--theme")?.as_str() {
                        "dark" => WorkspaceTheme::Dark,
                        "light" => WorkspaceTheme::Light,
                        _ => return Err("--theme must be dark or light".into()),
                    });
                }
                _ if arg.starts_with('-') => return Err(format!("Unknown option: {arg}")),
                _ => positional.push(arg),
            }
        }
        let mut positional = positional.into_iter();
        let output = positional.next().unwrap_or_else(|| "blueprint.png".into());
        let source = if fixture.is_none() {
            positional.next()
        } else {
            None
        };
        let positional_mode = positional.next();
        if positional.next().is_some() {
            return Err("Too many positional arguments; use --help for usage".into());
        }
        if requested_mode.is_some() && positional_mode.is_some() {
            return Err("Choose either a positional mode or --mode".into());
        }
        let mode = requested_mode
            .or(positional_mode)
            .unwrap_or_else(|| "none".into());
        if !matches!(
            mode.as_str(),
            "none"
                | "shape"
                | "column"
                | "focus"
                | "connector"
                | "all"
                | "drag"
                | "connect"
                | "packs"
                | "network"
                | "network-services"
                | "network-drop"
                | "network-transfer"
                | "deploy"
                | "calendar"
                | "sql-warning"
                | "long-text"
                | "error"
        ) {
            return Err(format!(
                "Unknown mode: {mode}; use --help for available modes"
            ));
        }
        let kind = source
            .as_deref()
            .and_then(|source| diagram_kind(source).ok());
        let file = source.filter(|_| kind.is_none()).map(PathBuf::from);
        Ok(Some(Self {
            output: output.into(),
            file,
            kind,
            fixture,
            mode,
            size,
            scale,
            fit,
            navigator,
            theme,
            preferences,
            catalogs,
            svg,
        }))
    }
}

fn next(args: &mut impl Iterator<Item = String>, option: &str) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("{option} needs a value; use --help for usage"))
}

fn diagram_kind(value: &str) -> Result<DiagramKind, String> {
    match value {
        "erd" => Ok(DiagramKind::Erd),
        "flowchart" => Ok(DiagramKind::Flowchart),
        "cloud" => Ok(DiagramKind::Cloud),
        _ => Err("--fixture must be erd, flowchart or cloud".into()),
    }
}

#[derive(Default)]
struct State {
    app: Option<BlueprintApp>,
    targets: fixtures::Targets,
}

type ScreenshotHarness<'a> = Harness<'a, State>;

fn app<'a>(harness: &'a mut ScreenshotHarness<'_>) -> &'a mut BlueprintApp {
    harness.state_mut().app.as_mut().expect("app initialized")
}

fn main() {
    let options = match Options::parse() {
        Ok(Some(options)) => options,
        Ok(None) => {
            println!("{HELP}");
            return;
        }
        Err(error) => {
            eprintln!("screenshot: {error}");
            std::process::exit(2);
        }
    };
    if let Err(error) = render(options) {
        eprintln!("screenshot: {error}");
        std::process::exit(1);
    }
}

fn render(options: Options) -> Result<(), Box<dyn std::error::Error>> {
    let startup = options.clone();
    let mut preferences = if let Some(path) = &options.preferences {
        WorkspacePreferences::from_json(&std::fs::read_to_string(path)?)
            .ok_or("Invalid or unsupported workspace preferences")?
    } else {
        WorkspacePreferences::default()
    };
    if let Some(theme) = options.theme {
        preferences.theme = theme;
    }
    if options.navigator {
        preferences.left_tab = LeftTab::Navigator;
    }
    let catalogs = if let Some(directory) = &options.catalogs {
        bp_icons::load_packs(directory)?
    } else if startup.fixture == Some(DiagramKind::Cloud) || startup.mode.starts_with("network-") {
        fixtures::cloud_catalogs()
    } else {
        Vec::new()
    };
    let mut harness = Harness::builder()
        .with_size(options.size)
        .with_pixels_per_point(options.scale)
        .with_os(egui::os::OperatingSystem::Nix)
        .with_step_dt(1.0 / 60.0)
        .wgpu()
        .build_ui_state(
            move |ui, state: &mut State| {
                if state.app.is_none() {
                    let mut app = BlueprintApp::new_with_workspace(
                        ui.ctx(),
                        startup.file.clone(),
                        preferences.clone(),
                        catalogs.clone(),
                    );
                    if let Some(kind) = startup.fixture.or(startup.kind) {
                        app.choose_page_kind(kind);
                    }
                    if let Some(kind) = startup.fixture {
                        state.targets = fixtures::populate(&mut app, kind);
                    }
                    state.app = Some(app);
                }
                state.app.as_mut().expect("app initialized").frame(ui);
            },
            State::default(),
        );
    harness.run();

    let network_targets = if options.mode.starts_with("network-") {
        Some(fixtures::network(
            app(&mut harness),
            options.mode == "network-transfer",
        ))
    } else {
        None
    };
    match options.mode.as_str() {
        "packs" => app(&mut harness).open_cloud_manager(),
        "network" | "deploy" => {
            app(&mut harness).add_starter_network();
            app(&mut harness).selection.clear();
            if options.mode == "deploy" {
                app(&mut harness).open_deployment();
            }
        }
        "calendar" => {
            app(&mut harness).cloud.version = "2026-07-31".into();
            app(&mut harness).open_cloud_manager();
        }
        "sql-warning" => {
            let app = app(&mut harness);
            app.paste_sql_schema();
            if let Some(import) = &mut app.sql.import {
                import.source = "CREATE TABLE incoming_orders (id uuid PRIMARY KEY);\nCREATE VIEW order_summary AS SELECT id FROM incoming_orders;".into();
            }
            app.preview_sql_import();
        }
        "error" => app(&mut harness).error = Some("The icon pack could not be imported. The ZIP contains no supported SVG service artwork. Choose the original provider ZIP and review its version and terms before trying again.".into()),
        _ => {}
    }
    if options.fit {
        app(&mut harness).fit_requested = true;
    }
    harness.run();

    let targets = harness.state().targets;
    let order: Vec<_> = {
        let app = app(&mut harness);
        app.doc
            .paint_order(app.page)
            .iter()
            .map(|element| (element.id, element.is_shape(), element.is_connector()))
            .collect()
    };
    let first_shape = targets
        .shape
        .or_else(|| order.iter().find(|e| e.1).map(|e| e.0));
    let first_connector = targets
        .connector
        .or_else(|| order.iter().find(|e| e.2).map(|e| e.0));
    if options.mode == "long-text"
        && let Some(id) = first_shape
    {
        app(&mut harness).apply("Long label diagnostic", [bp_commands::Command::Set {
            id,
            prop: bp_commands::Prop::Text("Reserve inventory and verify every shipment destination before processing the next order".into()),
        }]);
        app(&mut harness).selection = vec![id];
        if app(&mut harness).page_kind() == Some(DiagramKind::Erd) {
            app(&mut harness).selection.clear();
            app(&mut harness).auto_arrange_erd();
            app(&mut harness).selection = vec![id];
        }
    }
    match options.mode.as_str() {
        "shape" => app(&mut harness).selection = first_shape.into_iter().collect(),
        "connector" => app(&mut harness).selection = first_connector.into_iter().collect(),
        "column" => {
            if let Some(id) = first_shape {
                let column = app(&mut harness)
                    .doc
                    .elements
                    .get(&id)
                    .and_then(|element| element.as_shape())
                    .and_then(|shape| shape.erd.as_ref())
                    .and_then(|table| {
                        let columns = table.columns_sorted();
                        columns
                            .get(1)
                            .or_else(|| columns.first())
                            .map(|column| column.id)
                    });
                app(&mut harness).select_navigation_element(id, column);
            }
        }
        "focus" => {
            harness.get_by_label("Select").focus();
        }
        "all" => app(&mut harness).select_all(),
        "drag" => {
            if let Some(id) = first_shape {
                let app = app(&mut harness);
                let bounds = app.doc.elements[&id].as_shape().expect("shape").bounds;
                let from = app.view.to_screen(app.canvas_rect.min, bounds.center());
                let to = app
                    .view
                    .to_screen(app.canvas_rect.min, bounds.center() + Vec2::new(183.0, 2.0));
                press_and_move(&mut harness, from, to);
            }
        }
        "network-drop" => {
            let targets = network_targets.as_ref().unwrap();
            let editor = app(&mut harness);
            let bounds = editor.doc.elements[&targets.service]
                .as_shape()
                .unwrap()
                .bounds;
            let from = editor
                .view
                .to_screen(editor.canvas_rect.min, bounds.center());
            let to = editor
                .view
                .to_screen(editor.canvas_rect.min, targets.drop_point);
            press_and_move(&mut harness, from, to);
            assert!(
                app(&mut harness).drag.is_active(),
                "service drag must be active in the captured UI"
            );
        }
        "network-transfer" => {
            let id = network_targets.as_ref().unwrap().subnet;
            let editor = app(&mut harness);
            let original_parent = editor.doc.elements[&id].parent;
            let bounds = editor.doc.elements[&id].as_shape().unwrap().bounds;
            let from = editor.view.to_screen(
                editor.canvas_rect.min,
                bounds.origin() + Vec2::new(180.0, 24.0),
            );
            let to = editor.view.to_screen(
                editor.canvas_rect.min,
                bounds.origin() + Vec2::new(1280.0, 24.0),
            );
            press_and_move(&mut harness, from, to);
            harness.event(Event::PointerButton {
                pos: to,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            });
            harness.run_steps(2);
            assert_ne!(
                app(&mut harness).doc.elements[&id].parent,
                original_parent,
                "the captured subnet must have transferred to the destination VPC"
            );
        }
        "connect" => {
            if let Some(id) = first_shape {
                let app = app(&mut harness);
                app.refresh_scene();
                let east = app
                    .scene
                    .shape(id)
                    .expect("shape geometry")
                    .ports
                    .iter()
                    .find(|port| port.id.as_str() == "e")
                    .expect("east connection port")
                    .at;
                let from = app.view.to_screen(app.canvas_rect.min, east);
                let to = app
                    .view
                    .to_screen(app.canvas_rect.min, east + Vec2::new(160.0, 120.0));
                harness.hover_at(from);
                harness.step();
                press_and_move(&mut harness, from, to);
            }
        }
        _ => {}
    }
    harness.run_steps(2);
    // Selection can open the ERD relationship drawer and reduce the canvas.
    // Fit that settled layout, while preserving in-progress gesture previews.
    if options.fit && !matches!(options.mode.as_str(), "drag" | "connect" | "network-drop") {
        app(&mut harness).fit_requested = true;
        harness.run_steps(2);
    }
    if options.mode == "calendar" {
        harness.get_by_label("Choose pack release date").click();
        harness.run_steps(2);
    }
    let image = harness.render()?;
    if let Some(path) = &options.svg {
        let app = app(&mut harness);
        std::fs::write(
            path,
            bp_export::page_to_svg(&app.doc, app.page, &Default::default()),
        )?;
    }
    if let Some(parent) = options
        .output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    image.save(&options.output)?;
    println!(
        "Wrote {} ({}×{} pixels, {}×{} logical points, scale {})",
        options.output.display(),
        image.width(),
        image.height(),
        options.size.x,
        options.size.y,
        options.scale,
    );
    Ok(())
}

fn press_and_move(harness: &mut ScreenshotHarness<'_>, from: Pos2, to: Pos2) {
    harness.hover_at(from);
    harness.event(Event::PointerButton {
        pos: from,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    for step in 1..=10 {
        harness.hover_at(from + (to - from) * (step as f32 / 10.0));
    }
    harness.step();
}
