//! Review screen: shows one card at a time during a study session.
//! Placeholder layout only. Scheduling comes from `study/` and the card itself
//! from `db/`, so this screen still takes no state.

use eframe::egui;

/// Draws the review screen.
///
/// Takes no state yet: there is no card to show and the button does nothing.
/// The grade buttons (Again, Hard, Good, Easy) arrive with the SM-2 work.
pub(super) fn show(ui: &mut egui::Ui) {
    ui.vertical_centered(|ui| {
        ui.add_space(32.0);

        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_min_size(egui::vec2(420.0, 200.0));
            ui.centered_and_justified(|ui| {
                ui.label("The card front will be shown here");
            });
        });

        ui.add_space(16.0);
        // The click is deliberately ignored: revealing the answer needs the
        // session state that arrives with `study/`.
        let _ = ui.button("Show answer");
    });
}
