#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod canvas;
mod panels;

use std::path::PathBuf;

fn main() -> eframe::Result {
    let file = std::env::args_os().nth(1).map(PathBuf::from);

    // wgpu first, OpenGL as a fallback for VMs and old GPUs.
    // Set BLUEPRINT_RENDERER=glow or =wgpu to force one.
    let renderers = match std::env::var("BLUEPRINT_RENDERER").as_deref() {
        Ok("glow") => vec![eframe::Renderer::Glow],
        Ok("wgpu") => vec![eframe::Renderer::Wgpu],
        _ => vec![eframe::Renderer::Wgpu, eframe::Renderer::Glow],
    };

    let mut result = Ok(());
    for renderer in renderers {
        let options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_title("Blueprint")
                .with_app_id("blueprint")
                .with_inner_size([1280.0, 800.0])
                .with_min_inner_size([640.0, 400.0])
                .with_drag_and_drop(true),
            renderer,
            ..Default::default()
        };
        let file = file.clone();
        result = eframe::run_native(
            "Blueprint",
            options,
            Box::new(move |cc| Ok(Box::new(app::BlueprintApp::new(cc, file)))),
        );
        match &result {
            Ok(()) => return Ok(()),
            Err(e) => eprintln!("Blueprint: the {renderer} renderer failed: {e}"),
        }
    }
    result
}