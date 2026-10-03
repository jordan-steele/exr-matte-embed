#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;

use clap::Parser;
use eframe::egui;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(version, about = "EXR Matte Embed desktop application")]
pub struct Args {
    /// Open a source folder in the desktop application.
    #[arg(long)]
    source: Option<PathBuf>,
    #[arg(long)]
    output_root: Option<PathBuf>,
    #[arg(long)]
    scan: bool,
    /// Alternate preference file, useful for independent review sessions.
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long, hide = true)]
    worker: bool,
    #[arg(long, hide = true)]
    run: bool,
    #[arg(long, hide = true)]
    capture_dir: Option<PathBuf>,
    #[arg(long, hide = true)]
    exit_after_capture: bool,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    if args.worker {
        return exr_matte_embed::batch::worker_main();
    }
    anyhow::ensure!(
        !args.run || args.source.is_some(),
        "--run requires an explicit --source"
    );
    anyhow::ensure!(
        !args.exit_after_capture || args.capture_dir.is_some(),
        "--exit-after-capture requires --capture-dir"
    );
    let png = image::load_from_memory(include_bytes!("../images/icon.png"))?.into_rgba8();
    let icon = egui::IconData {
        width: png.width(),
        height: png.height(),
        rgba: png.into_raw(),
    };
    let native = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("EXR Matte Embed")
            .with_inner_size([1180.0, 860.0])
            .with_min_inner_size([900.0, 680.0])
            .with_icon(icon),
        renderer: eframe::Renderer::Glow,
        centered: true,
        ..Default::default()
    };
    eframe::run_native(
        "EXR Matte Embed",
        native,
        Box::new(move |cc| Ok(Box::new(app::MatteApp::new(cc, args)))),
    )
    .map_err(|error| anyhow::anyhow!("Could not open the app: {error}"))
}
