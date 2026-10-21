#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod audio;
mod ui;

fn main() -> eframe::Result {
    let arguments: Vec<String> = std::env::args().collect();
    if arguments.iter().any(|arg| arg == "--version") {
        println!("GLENN Extended {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let capture = ui::Capture::from_args(&arguments);
    let size = capture.as_ref().map_or([1440.0, 900.0], |c| c.size);
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icons/icon.png")).ok();
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_title("GLENN Extended · аэродинамическая лаборатория")
        .with_inner_size(size)
        .with_min_inner_size([640.0, 480.0]);
    if let Some(icon) = icon {
        viewport = viewport.with_icon(icon);
    }
    eframe::run_native(
        "GLENN Extended",
        eframe::NativeOptions {
            viewport,
            persist_window: capture.is_none(),
            ..Default::default()
        },
        Box::new(move |creation| Ok(Box::new(ui::Workbench::new(creation, capture)))),
    )
}
