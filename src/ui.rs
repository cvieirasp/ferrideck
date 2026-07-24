//! Presentation layer: windows, screens and widgets of the desktop app.
//! Renders application state and turns user input into calls to `study`, `db`
//! and `sync`. Never runs SQL or HTTP itself.

use eframe::egui;

/// Root of the application UI and the single owner of all UI state.
///
/// In immediate mode there is no persistent widget tree: every frame is
/// redrawn from this struct, so anything the screen shows must be reachable
/// from here. It is empty for now and grows as screens are added.
pub struct FerrideckApp;

impl FerrideckApp {
    /// Builds the application from eframe's creation context.
    ///
    /// eframe hands over the context before the first frame, which is where
    /// egui-wide setup belongs (custom fonts, visual style, restoring persisted
    /// state). Nothing is customized yet, so the context is unused.
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self
    }
}

impl eframe::App for FerrideckApp {
    /// Draws one frame. Called by eframe on every repaint, so the work done
    /// here must stay cheap.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.centered_and_justified(|ui| {
                ui.heading("Ferrideck");
            });
        });
    }
}
