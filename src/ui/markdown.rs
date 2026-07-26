//! Markdown rendering for card content.
//!
//! Card fronts and backs are stored as Markdown source (see
//! `docs/decisions/0003-markdown-rendering.md`), and this module is the only
//! place that turns that source into egui widgets. Keeping it behind one
//! function means no other screen names `egui_commonmark`, so replacing the
//! renderer later is a change to this file and a dependency line.

use eframe::egui;
use egui_commonmark::CommonMarkViewer;
use std::borrow::Cow;

pub(super) use egui_commonmark::CommonMarkCache;

/// Draws `text` as rendered Markdown into `ui`.
///
/// `cache` has to be the same value on every frame: it is what the renderer
/// remembers between them. See the field on `FerrideckApp` for what it holds.
///
/// The viewer is built fresh on each call because it is a builder holding
/// options, not state, and `show` consumes it. All the state lives in `cache`.
///
/// The source passes through [`with_hard_line_breaks`] first, so an Enter key
/// press in the editor is a line break here.
///
/// ```ignore
/// render_markdown(ui, cache, "**bold**, *italic* and a list:\n\n- one\n- two");
/// ```
pub(super) fn render_markdown(ui: &mut egui::Ui, cache: &mut CommonMarkCache, text: &str) {
    CommonMarkViewer::new().show(ui, cache, &with_hard_line_breaks(text));
}

/// Rewrites `source` so that every line break the user typed survives rendering.
///
/// CommonMark treats a single newline inside a paragraph as a *soft* break and
/// renders it as a space, so that a document can be wrapped in its source
/// without changing its output. That rule is written for prose files, and it is
/// the wrong one for a card editor: here the newline is not an accident of
/// wrapping, it is the user pressing Enter to put "am / is / are" on three
/// lines. A *hard* break is what they mean, and CommonMark spells it as a line
/// ending in two spaces, which is what this adds.
///
/// The alternative was to teach the user the two-space rule, or to store the
/// spaces in the database. Both were rejected: the first is a footgun with no
/// visible cause, and the second would put a rendering detail into the stored
/// content, which `db/` and `sync/` would then carry around forever. This runs
/// between storage and the renderer, so what is saved stays exactly what was
/// typed.
///
/// Lines are left alone when the break would change nothing or would corrupt
/// content:
///
/// - the last line, which has no newline after it;
/// - a blank line, and any line followed by one: the block already ends there;
/// - a line already ending in two spaces or a backslash: it is a hard break;
/// - anything inside a fenced code block, where newlines are literal already,
///   so there is no soft break to fix and appending spaces would edit the code;
/// - the fence lines themselves and the line right before one, which are block
///   boundaries rather than text inside a paragraph.
///
/// Indented code blocks are not detected, because telling one apart from a
/// wrapped list item needs a real block parser. The cost of missing them is two
/// invisible trailing spaces inside the code, not a change in structure.
///
/// Returns [`Cow`] because the common case, a card front with no newline at
/// all, needs no new `String`. This runs on every field on every frame.
///
/// ```ignore
/// assert_eq!(with_hard_line_breaks("am\nis"), "am  \nis");
/// ```
fn with_hard_line_breaks(source: &str) -> Cow<'_, str> {
    // Nothing to break: hand back the borrowed source and allocate nothing.
    if !source.contains('\n') {
        return Cow::Borrowed(source);
    }

    let mut out = String::with_capacity(source.len());
    let mut inside_code_fence = false;

    // `lines` splits on `\n` and drops a trailing `\r`, so text that arrived
    // with Windows line endings is handled without a special case. Rejoining
    // with `\n` only affects the copy being rendered.
    let mut lines = source.lines().peekable();

    while let Some(line) = lines.next() {
        out.push_str(line);

        // No newline follows the last line, so there is no break to harden.
        let Some(next) = lines.peek() else {
            break;
        };

        // A fence line is block structure, and so is the line right before one:
        // the paragraph ends where the code block begins. Neither takes a break,
        // which is why this is read before the state below is toggled.
        let borders_a_fence = is_code_fence(line) || is_code_fence(next);

        if is_code_fence(line) {
            inside_code_fence = !inside_code_fence;
        }

        if !inside_code_fence && !borders_a_fence && needs_hard_break(line, next) {
            out.push_str("  ");
        }

        out.push('\n');
    }

    Cow::Owned(out)
}

/// Whether a newline between these two lines has to be made explicit.
fn needs_hard_break(line: &str, next: &str) -> bool {
    // A blank line on either side is a block boundary: the paragraph ends
    // there on its own and a break would have nothing to join.
    if line.trim().is_empty() || next.trim().is_empty() {
        return false;
    }

    // Already a hard break in the source, written either way CommonMark
    // allows. Adding to it would be harmless but pointless.
    !line.ends_with("  ") && !line.ends_with('\\')
}

/// Whether the line opens or closes a fenced code block.
fn is_code_fence(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("```") || trimmed.starts_with("~~~")
}

/// Renders Markdown with the renderer's paragraph style swapped for `style`.
///
/// The renderer draws ordinary paragraphs in `TextStyle::Body`. Remapping that
/// one entry inside a `ui.scope` is what lets a card front keep the heading size
/// it had as a plain label, and an example stay visually secondary, without a
/// font size being hard-coded into this module. Emphasis and code spans are
/// derived from the same entry, so they scale along with it.
///
/// The scope restores the style when it ends, so nothing drawn afterwards is
/// affected.
///
/// This lives here rather than on the review screen because it is part of what
/// "rendered card content" means in this app: the editor's preview has to pick
/// the same style for the same field, or it would be previewing a document the
/// user is never shown.
///
/// ```ignore
/// render_markdown_as(ui, cache, &egui::TextStyle::Heading, &card.front);
/// ```
pub(super) fn render_markdown_as(
    ui: &mut egui::Ui,
    cache: &mut CommonMarkCache,
    style: &egui::TextStyle,
    text: &str,
) {
    ui.scope(|ui| {
        let font = style.resolve(ui.style());
        ui.style_mut()
            .text_styles
            .insert(egui::TextStyle::Body, font);
        render_markdown(ui, cache, text);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rewritten source, as a `String`, for readable assertions.
    fn hardened(source: &str) -> String {
        with_hard_line_breaks(source).into_owned()
    }

    #[test]
    fn text_without_newlines_is_borrowed_unchanged() {
        let source = "The **cat** sat on the *mat*";

        let result = with_hard_line_breaks(source);

        assert_eq!(result, source);
        // No allocation for the common case: a one-line card front.
        assert!(matches!(result, Cow::Borrowed(_)));
    }

    #[test]
    fn a_single_newline_becomes_a_hard_break() {
        assert_eq!(hardened("am\nis\nare"), "am  \nis  \nare");
    }

    #[test]
    fn windows_line_endings_are_handled_too() {
        assert_eq!(hardened("am\r\nis"), "am  \nis");
    }

    #[test]
    fn blank_lines_keep_separating_paragraphs() {
        // A blank line already ends the block, so nothing is added around it
        // and the two paragraphs stay two paragraphs.
        assert_eq!(hardened("first\n\nsecond"), "first\n\nsecond");
        assert_eq!(hardened("Forms:\n\n- am\n- is"), "Forms:\n\n- am  \n- is");
    }

    #[test]
    fn the_last_line_gains_nothing() {
        // There is no newline after it, so a break would only be trailing
        // whitespace.
        assert_eq!(hardened("only\ntwo"), "only  \ntwo");
        assert!(!hardened("only\ntwo").ends_with(' '));
    }

    #[test]
    fn existing_hard_breaks_are_left_as_they_are() {
        // Written the two-space way and the backslash way. Neither is doubled.
        assert_eq!(hardened("am  \nis"), "am  \nis");
        assert_eq!(hardened("am\\\nis"), "am\\\nis");
    }

    #[test]
    fn fenced_code_blocks_are_not_edited() {
        let source = "Example:\n\n```rust\nlet a = 1;\nlet b = 2;\n```\n\ndone";

        // Every line between the fences survives byte for byte: inside a code
        // block the newline is already literal, so there is nothing to fix.
        assert_eq!(hardened(source), source);
    }

    #[test]
    fn text_around_a_code_block_still_gets_breaks() {
        let source = "before\nstill before\n```\ncode\n```\nafter\nstill after";

        assert_eq!(
            hardened(source),
            "before  \nstill before\n```\ncode\n```\nafter  \nstill after"
        );
    }

    #[test]
    fn an_unclosed_fence_does_not_leak_into_the_rest() {
        // Everything after the opening fence is code as far as the renderer is
        // concerned, so it is left alone rather than half-rewritten.
        assert_eq!(hardened("```\ncode\nmore code"), "```\ncode\nmore code");
    }

    #[test]
    fn empty_input_is_borrowed_and_stays_empty() {
        let result = with_hard_line_breaks("");

        assert_eq!(result, "");
        assert!(matches!(result, Cow::Borrowed(_)));
    }
}
