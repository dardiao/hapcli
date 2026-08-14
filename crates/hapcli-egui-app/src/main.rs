#![windows_subsystem = "windows"]

mod app;
mod connect;
mod forward;
mod keys;
mod render;
mod profiles;
mod quick;
mod settings;
mod sftp;
mod terminal;
mod trzsz;
mod update;

use eframe::egui;

fn main() -> eframe::Result {
    let settings = settings::load_settings();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1000.0, 680.0])
            .with_min_inner_size([420.0, 260.0])
            .with_title("hapcli")
            .with_icon(app_icon())
            .with_transparent(settings.transparent_window),
        ..Default::default()
    };

    eframe::run_native(
        "hapcli",
        options,
        Box::new(|cc| Ok(Box::new(app::HapcliApp::new(cc)?))),
    )
}

/// 内嵌应用图标（与 .app 打包图标同一份生成物），
/// 裸二进制运行（cargo run）时也能显示在 Dock / 标题栏。
fn app_icon() -> std::sync::Arc<egui::IconData> {
    const ICON_BYTES: &[u8] = include_bytes!("../assets/icon.png");
    let rgba = image::load_from_memory(ICON_BYTES)
        .expect("embedded app icon must be a valid PNG")
        .to_rgba8();
    let resized = image::imageops::resize(
        &rgba,
        64,
        64,
        image::imageops::FilterType::Lanczos3,
    );
    let (width, height) = resized.dimensions();
    std::sync::Arc::new(egui::IconData {
        rgba: resized.into_raw(),
        width,
        height,
    })
}
