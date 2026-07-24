//! Markdown rendering for card content.
//!
//! Card fronts and backs are stored as Markdown source (see
//! `docs/decisions/0003-markdown-rendering.md`), and this module is the only
//! place that turns that source into egui widgets. Keeping it behind one
//! function means no other screen names `egui_commonmark`, so replacing the
//! renderer later is a change to this file and a dependency line.

use eframe::egui;
use egui_commonmark::CommonMarkViewer;

pub(super) use egui_commonmark::CommonMarkCache;

/// Draws `text` as rendered Markdown into `ui`.
///
/// `cache` has to be the same value on every frame: it is what the renderer
/// remembers between them. See the field on `FerrideckApp` for what it holds.
///
/// The viewer is built fresh on each call because it is a builder holding
/// options, not state, and `show` consumes it. All the state lives in `cache`.
///
/// ```ignore
/// render_markdown(ui, cache, "**bold**, *italic* and a list:\n\n- one\n- two");
/// ```
pub(super) fn render_markdown(ui: &mut egui::Ui, cache: &mut CommonMarkCache, text: &str) {
    CommonMarkViewer::new().show(ui, cache, text);
}
