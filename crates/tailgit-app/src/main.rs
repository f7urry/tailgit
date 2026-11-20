//! TailGIT: a desktop Git client modeled on IntelliJ's Git tool window.
//!
//! Usage: `tailgit [path-to-repository]`

mod app;
mod brand;
mod jobs;
mod menubar;
mod ops;
mod repo;
mod settings;
mod theme;
mod ui;

use eframe::egui;

fn main() -> eframe::Result {
    // Build helper used by scripts/bundle-macos.sh.
    let args: Vec<_> = std::env::args_os().collect();
    if args.get(1).is_some_and(|a| a == "--export-icon") {
        let out = args.get(2).map(std::path::PathBuf::from).unwrap_or_else(|| "icon_1024.png".into());
        if let Err(e) = brand::export_png(&out) {
            eprintln!("export failed: {e}");
            std::process::exit(1);
        }
        return Ok(());
    }
    let initial = std::env::args_os().nth(1).map(std::path::PathBuf::from);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("TailGIT")
            .with_icon(std::sync::Arc::new(brand::app_icon()))
            .with_inner_size([1500.0, 900.0])
            .with_min_inner_size([900.0, 500.0])
            .with_fullsize_content_view(true)
            .with_title_shown(false)
            .with_titlebar_shown(false),
        // OpenGL keeps up with live window resizing on macOS; wgpu rebuilt the swapchain every frame and lagged.
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        "TailGIT",
        options,
        Box::new(move |cc| {
            menubar::install();
            theme::install(&cc.egui_ctx, &settings::Settings::load());
            Ok(Box::new(app::TailApp::new(cc, initial)))
        }),
    )
}
