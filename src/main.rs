mod db;
mod models;
mod study;
mod sync;
mod ui;

use anyhow::{Context, Result};
use eframe::egui;
use ui::FerrideckApp;

fn main() -> Result<()> {
    // Opened before the window so a broken database reports a readable error
    // instead of an empty UI, then handed to the app, which owns it for the
    // rest of the process.
    let connection = db::init().context("starting Ferrideck")?;

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([900.0, 600.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Ferrideck",
        options,
        Box::new(move |_cc| Ok(Box::new(FerrideckApp::new(connection)))),
    )
    .context("running the application window")?;

    Ok(())
}
