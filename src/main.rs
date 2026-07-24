mod db;
mod models;
mod study;
mod sync;
mod ui;

use eframe::egui;
use ui::FerrideckApp;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([900.0, 600.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Ferrideck",
        options,
        Box::new(|_cc| Ok(Box::new(FerrideckApp::default()))),
    )
}
