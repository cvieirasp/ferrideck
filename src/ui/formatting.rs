//! The string half of the editor's formatting shortcuts.
//!
//! Ctrl+B and Ctrl+I are two things stacked on top of each other: a rule about
//! text ("put `**` around what is selected, or take it away again") and a
//! conversation with egui about focus, key events and cursors. This module is
//! the first one only. It knows nothing about egui, allocates a `String` and
//! hands it back, and is therefore testable with plain `assert_eq!` instead of
//! a running window. The wiring lives in `ui/card_editor.rs`.
//!
//! # Positions are character indices, not byte offsets
//!
//! Every index in this module counts *characters*, matching egui, whose
//! `CCursor::index` is a `CharIndex` and is documented as "character offset
//! (NOT byte offset!)".
//!
//! The distinction is not pedantic here. A Rust `String` is UTF-8: one `char`
//! is one Unicode scalar value, but it occupies one to four bytes. In
//! `"pronúncia"` the `ú` is two bytes, so the word is 9 characters and 10
//! bytes, and every position after the accent disagrees between the two
//! counts. Slicing is by byte, and `&text[4..5]` on that word would cut the
//! `ú` in half. Rust refuses to hand out a `&str` that is not valid UTF-8, and
//! since the slice operator cannot return an error it **panics**:
//! `byte index 5 is not a char boundary`. Our cards are full of Portuguese and
//! that panic would be a crash in the editor, not a rounding error.
//!
//! So the boundary is drawn once, on the way in: the character indices egui
//! gives us are turned into byte offsets by [`byte_offset`], which walks the
//! string with `char_indices` and can only ever land on a real boundary. From
//! there on the code slices freely, and the ranges handed back are converted to
//! characters again by construction, because they are built from the character
//! positions we started with plus the character width of the marker.

use std::ops::Range;

/// Marker Ctrl+B toggles.
pub(super) const BOLD: &str = "**";

/// Marker Ctrl+I toggles.
pub(super) const ITALIC: &str = "*";

/// Wraps the selection in `marker`, or removes the marker if it is already
/// there.
///
/// `selection` and the returned range are character indices into `text` and
/// into the returned string respectively; see the module documentation for why
/// that matters. The returned range is what the caller should select
/// afterwards, so that the same words stay selected across the edit and a
/// second press can undo the first.
///
/// Whitespace at the edges of the selection is left out of it before anything
/// else happens, because CommonMark does not read `**word **` as bold. See
/// [`without_edge_whitespace`].
///
/// The three outcomes, in the order they are tried:
///
/// 1. **Nothing selected.** An empty pair is inserted and the returned range is
///    the empty range between the two markers: the user pressed Ctrl+B to
///    *start* writing in bold, so the cursor lands where the bold text goes.
/// 2. **Already wrapped.** The markers come off. They count as "already there"
///    whether they sit inside the selection (the user selected `**word**`,
///    markers and all) or immediately outside it (the user double-clicked
///    `word` between them), because both are how a person would select bold
///    text without thinking about it.
/// 3. **Anything else.** The selection is wrapped.
///
/// # Telling `*` and `**` apart
///
/// Case 2 needs care, because `*` is a prefix of `**`. Reading "is there a `*`
/// just before the selection?" naively, Ctrl+I on the `bold` inside `**bold**`
/// would find the second asterisk of the opening `**`, take one off each side
/// and silently turn bold text into italic.
///
/// The rule used instead is that a marker matches only when the run of marker
/// characters at that boundary is *exactly* as long as the marker. Around
/// `**bold**`, Ctrl+I sees a run of two where it wants one, decides that this
/// is somebody else's markup, and nests rather than unwrapping:
/// `***bold***`, which CommonMark reads as bold *and* italic. That is what the
/// user asked for.
///
/// The known cost of the rule is the other direction: on `***both***`, already
/// bold and italic, the run is three and neither shortcut recognises it, so
/// both nest further instead of peeling one layer off. Unwrapping that case
/// correctly needs to know which marker is the inner one, which the source text
/// does not say. Nesting is the safer failure: it is visible, it is undone by
/// Ctrl+Z, and it never rewrites markup the user did not aim at.
///
/// ```ignore
/// let (text, selection) = toggle_wrap("cat sat", 0..3, BOLD);
/// assert_eq!(text, "**cat** sat");
/// assert_eq!(selection, 2..5);
/// ```
pub(super) fn toggle_wrap(
    text: &str,
    selection: Range<usize>,
    marker: &str,
) -> (String, Range<usize>) {
    let width = marker.chars().count();

    // No marker, nothing to toggle. Unreachable through the shortcuts, whose
    // markers are the constants above, but it keeps the function total instead
    // of leaving a subtraction below to underflow.
    if width == 0 {
        return (text.to_owned(), selection);
    }

    let selection = usable(text, selection);
    let Range { start, end } = without_edge_whitespace(text, selection);

    // The one conversion from characters to bytes. Everything below is byte
    // slicing on offsets that are known to be char boundaries.
    let start_byte = byte_offset(text, start);
    let end_byte = byte_offset(text, end);
    let (before, rest) = text.split_at(start_byte);
    let (selected, after) = rest.split_at(end_byte - start_byte);

    if selected.is_empty() {
        return (
            format!("{before}{marker}{marker}{after}"),
            start + width..start + width,
        );
    }

    if let Some(inner) = without_inner_markers(selected, marker) {
        // Two markers left the string, both from inside the selection.
        return (format!("{before}{inner}{after}"), start..end - 2 * width);
    }

    if let Some((before, after)) = without_outer_markers(before, after, marker) {
        // One marker left the string before the selection, so both ends of the
        // selection move back by its width and the same text stays selected.
        return (
            format!("{before}{selected}{after}"),
            start - width..end - width,
        );
    }

    (
        format!("{before}{marker}{selected}{marker}{after}"),
        start + width..end + width,
    )
}

/// The selection as a range this module can slice with.
///
/// Two things are fixed here. egui stores a selection as a primary and a
/// secondary cursor in the order the user dragged them, so selecting right to
/// left gives `end < start`; and a cursor is stored across frames while the
/// text is free to change under it, so it can point past the end. Both are
/// normal, neither is an error worth reporting, and both would be a panic three
/// lines later.
fn usable(text: &str, selection: Range<usize>) -> Range<usize> {
    let length = text.chars().count();
    let start = selection.start.min(selection.end).min(length);
    let end = selection.start.max(selection.end).min(length);
    start..end
}

/// The selection with the whitespace at its edges left out of it.
///
/// CommonMark only reads a marker as emphasis when it touches the text it
/// emphasises: the opening one may not be followed by a space and the closing
/// one may not be preceded by one. So `**word **` and `** word**` are not bold,
/// they are literal asterisks. A selection that carries a space is easy to
/// make - dragging a little past a word, or selecting a whole line - and
/// wrapping it as it stands would produce exactly that, markup that renders as
/// itself.
///
/// Moving the edges inwards puts the markers against the word and leaves the
/// spaces outside, where the user left them. It also makes the toggle work from
/// the same sloppy selection: `**word** ` picked up with its trailing space is
/// still recognised as wrapped, and the markers come off.
///
/// A selection that is nothing but whitespace collapses to its start, and the
/// empty-selection rule then inserts a pair there. The spaces are never
/// swallowed.
fn without_edge_whitespace(text: &str, selection: Range<usize>) -> Range<usize> {
    let start_byte = byte_offset(text, selection.start);
    let end_byte = byte_offset(text, selection.end);
    let selected = &text[start_byte..end_byte];

    let leading = selected.chars().take_while(|c| c.is_whitespace()).count();
    let trailing = selected
        .chars()
        .rev()
        .take_while(|c| c.is_whitespace())
        .count();

    // The two counts overlap only when every character is whitespace, which is
    // also the case where there is nothing left to wrap.
    if leading + trailing >= selected.chars().count() {
        return selection.start..selection.start;
    }

    selection.start + leading..selection.end - trailing
}

/// Byte offset of the character at `index`, or the length of the text if there
/// is no such character.
///
/// `char_indices` yields the byte offset of every character, so the result is
/// always a char boundary and slicing at it cannot panic.
fn byte_offset(text: &str, index: usize) -> usize {
    text.char_indices()
        .nth(index)
        .map_or(text.len(), |(offset, _)| offset)
}

/// The selection without the markers it carries at both ends.
///
/// `strip_suffix` applied to the result of `strip_prefix` also rules out the
/// two markers being the same characters read twice: `"**"` is not an empty
/// string in bold.
fn without_inner_markers<'t>(selected: &'t str, marker: &str) -> Option<&'t str> {
    let inner = selected.strip_prefix(marker)?.strip_suffix(marker)?;

    (starts_with_exactly(selected, marker) && ends_with_exactly(selected, marker)).then_some(inner)
}

/// The text on either side of the selection, with the markers that wrap it
/// removed.
fn without_outer_markers<'t>(
    before: &'t str,
    after: &'t str,
    marker: &str,
) -> Option<(&'t str, &'t str)> {
    if !ends_with_exactly(before, marker) || !starts_with_exactly(after, marker) {
        return None;
    }

    Some((before.strip_suffix(marker)?, after.strip_prefix(marker)?))
}

/// Whether `text` opens with this marker and not with a longer run of its
/// character.
///
/// This is the `*` versus `**` rule from [`toggle_wrap`]. Markers are runs of a
/// single character, so counting that character is enough to measure the run.
fn starts_with_exactly(text: &str, marker: &str) -> bool {
    let Some(character) = marker.chars().next() else {
        return false;
    };

    text.chars().take_while(|found| *found == character).count() == marker.chars().count()
}

/// Whether `text` ends with this marker and not with a longer run of its
/// character.
fn ends_with_exactly(text: &str, marker: &str) -> bool {
    let Some(character) = marker.chars().next() else {
        return false;
    };

    text.chars()
        .rev()
        .take_while(|found| *found == character)
        .count()
        == marker.chars().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The characters of `text` covered by `range`.
    ///
    /// Character indices again, so this is what the user would see highlighted
    /// after the edit, not a byte slice that would cut an accent in half.
    fn covered(text: &str, range: Range<usize>) -> String {
        text.chars()
            .skip(range.start)
            .take(range.end - range.start)
            .collect()
    }

    #[test]
    fn an_empty_selection_opens_a_pair_around_the_cursor() {
        // The cursor ends up between the markers, so the next keystroke is
        // already bold.
        assert_eq!(toggle_wrap("", 0..0, BOLD), ("****".to_owned(), 2..2));
        assert_eq!(toggle_wrap("", 0..0, ITALIC), ("**".to_owned(), 1..1));

        assert_eq!(
            toggle_wrap("cat sat", 4..4, BOLD),
            ("cat ****sat".to_owned(), 6..6)
        );
    }

    #[test]
    fn a_single_word_gains_markers_and_stays_selected() {
        assert_eq!(
            toggle_wrap("cat sat", 0..3, BOLD),
            ("**cat** sat".to_owned(), 2..5)
        );

        // The returned range covers the same word it did before the edit, so
        // the word stays highlighted and a second Ctrl+B undoes the first.
        let (text, selection) = toggle_wrap("cat sat", 0..3, BOLD);
        assert_eq!(covered(&text, selection), "cat");
    }

    #[test]
    fn a_multi_word_selection_is_wrapped_as_one() {
        assert_eq!(
            toggle_wrap("the cat sat", 4..11, ITALIC),
            ("the *cat sat*".to_owned(), 5..12)
        );
    }

    #[test]
    fn a_selection_at_either_end_of_the_text_is_wrapped_too() {
        // Nothing before the selection: `before` is empty.
        assert_eq!(
            toggle_wrap("cat sat", 0..7, BOLD),
            ("**cat sat**".to_owned(), 2..9)
        );

        // Nothing after it: `after` is empty.
        assert_eq!(
            toggle_wrap("cat sat", 4..7, BOLD),
            ("cat **sat**".to_owned(), 6..9)
        );
    }

    #[test]
    fn markers_inside_the_selection_come_off() {
        // The user selected the word together with its markers.
        assert_eq!(
            toggle_wrap("**cat** sat", 0..7, BOLD),
            ("cat sat".to_owned(), 0..3)
        );

        assert_eq!(
            toggle_wrap("the *cat sat*", 4..13, ITALIC),
            ("the cat sat".to_owned(), 4..11)
        );
    }

    #[test]
    fn markers_outside_the_selection_come_off() {
        // The user double-clicked the word between the markers.
        assert_eq!(
            toggle_wrap("**cat** sat", 2..5, BOLD),
            ("cat sat".to_owned(), 0..3)
        );

        assert_eq!(
            toggle_wrap("the *cat sat*", 5..12, ITALIC),
            ("the cat sat".to_owned(), 4..11)
        );
    }

    #[test]
    fn the_markers_are_put_against_the_text_not_against_the_spaces() {
        // `**cat **` is not bold in CommonMark, it is three asterisks and a
        // word, so the space stays outside the markers.
        assert_eq!(
            toggle_wrap("the cat sat", 4..8, BOLD),
            ("the **cat** sat".to_owned(), 6..9)
        );

        // Same on the other side, and on both at once.
        assert_eq!(
            toggle_wrap("the cat sat", 3..7, BOLD),
            ("the **cat** sat".to_owned(), 6..9)
        );
        assert_eq!(
            toggle_wrap("the cat sat", 3..8, ITALIC),
            ("the *cat* sat".to_owned(), 5..8)
        );

        // A line selected whole, trailing newline and all.
        assert_eq!(
            toggle_wrap("cat\n", 0..4, BOLD),
            ("**cat**\n".to_owned(), 2..5)
        );
    }

    #[test]
    fn a_selection_with_spaces_still_toggles_the_markers_off() {
        // The selection the user actually makes when they drag over a bold word
        // is rarely exactly the word: it picks up the spaces around it. That has
        // to count as "already bold", or the shortcut wraps what it should
        // unwrap.
        assert_eq!(
            toggle_wrap("the **cat** sat", 3..12, BOLD),
            ("the cat sat".to_owned(), 4..7)
        );

        assert_eq!(
            toggle_wrap("**cat** ", 0..8, BOLD),
            ("cat ".to_owned(), 0..3)
        );
    }

    #[test]
    fn a_selection_of_only_whitespace_inserts_a_pair() {
        // Nothing to emphasise, so this is an empty selection with extra steps.
        // The spaces are left where they are.
        assert_eq!(
            toggle_wrap("a   b", 1..4, BOLD),
            ("a****   b".to_owned(), 3..3)
        );
    }

    #[test]
    fn wrapping_and_toggling_again_returns_the_original() {
        let original = "the cat sat";

        let (bold, selection) = toggle_wrap(original, 4..7, BOLD);
        assert_eq!(bold, "the **cat** sat");

        // The range that came out is fed straight back in, which is exactly
        // what pressing Ctrl+B twice does.
        assert_eq!(
            toggle_wrap(&bold, selection, BOLD),
            (original.to_owned(), 4..7)
        );
    }

    #[test]
    fn accented_text_is_wrapped_by_character_not_by_byte() {
        // "pronúncia" is 9 characters and 10 bytes: the ú takes two. Selecting
        // only the accented character puts the boundary in the middle of it if
        // the indices are read as bytes, which panics rather than misbehaving.
        assert_eq!(
            toggle_wrap("pronúncia", 4..5, ITALIC),
            ("pron*ú*ncia".to_owned(), 5..6)
        );

        let (text, selection) = toggle_wrap("a pronúncia é clara", 2..11, BOLD);
        assert_eq!(text, "a **pronúncia** é clara");
        // The range is past two multi-byte characters by the time it ends, so
        // reading it as bytes would highlight the wrong word.
        assert_eq!(covered(&text, selection), "pronúncia");
    }

    #[test]
    fn accented_text_toggles_off_by_character_too() {
        // Every index here is past at least one multi-byte character.
        assert_eq!(
            toggle_wrap("a **pronúncia** é clara", 4..13, BOLD),
            ("a pronúncia é clara".to_owned(), 2..11)
        );

        assert_eq!(
            toggle_wrap("a **pronúncia** é clara", 2..15, BOLD),
            ("a pronúncia é clara".to_owned(), 2..11)
        );
    }

    #[test]
    fn bold_markers_are_not_mistaken_for_an_italic_one() {
        // The asterisks around "cat" are a run of two, so Ctrl+I does not
        // recognise them as its own and nests instead of unwrapping. The result
        // is bold and italic, which is what asking for italic inside bold
        // means.
        assert_eq!(
            toggle_wrap("**cat**", 2..5, ITALIC),
            ("***cat***".to_owned(), 3..6)
        );

        // Same when the bold markers are inside the selection.
        assert_eq!(
            toggle_wrap("**cat**", 0..7, ITALIC),
            ("***cat***".to_owned(), 1..8)
        );
    }

    #[test]
    fn an_italic_marker_is_not_mistaken_for_a_bold_one() {
        assert_eq!(
            toggle_wrap("*cat*", 1..4, BOLD),
            ("***cat***".to_owned(), 3..6)
        );

        assert_eq!(
            toggle_wrap("*cat*", 0..5, BOLD),
            ("***cat***".to_owned(), 2..7)
        );
    }

    #[test]
    fn text_that_is_already_bold_and_italic_nests_further() {
        // The documented limitation: a run of three matches neither marker, so
        // the shortcut wraps rather than peeling off the layer it owns. Pinned
        // here so a future change to the rule is a deliberate one.
        assert_eq!(
            toggle_wrap("***cat***", 3..6, BOLD),
            ("*****cat*****".to_owned(), 5..8)
        );
    }

    #[test]
    fn a_selection_that_is_too_short_to_hold_markers_is_wrapped() {
        // "**" is not an empty string in bold, so it gains a pair rather than
        // losing itself.
        assert_eq!(toggle_wrap("**", 0..2, BOLD), ("******".to_owned(), 2..4));
    }

    #[test]
    fn a_backwards_or_out_of_range_selection_does_not_panic() {
        // Dragged right to left, which is how egui reports it. Written the long
        // way because `3..0` as a literal is a lint: an empty range is almost
        // always a mistake, and here it is the input we are defending against.
        let backwards = Range { start: 3, end: 0 };
        assert_eq!(
            toggle_wrap("cat sat", backwards, BOLD),
            ("**cat** sat".to_owned(), 2..5)
        );

        // Stale cursor pointing past the end of a text that has since shrunk.
        assert_eq!(
            toggle_wrap("cat", 0..999, BOLD),
            ("**cat**".to_owned(), 2..5)
        );
        assert_eq!(toggle_wrap("", 7..7, BOLD), ("****".to_owned(), 2..2));
    }

    #[test]
    fn an_empty_marker_changes_nothing() {
        assert_eq!(toggle_wrap("cat", 0..3, ""), ("cat".to_owned(), 0..3));
    }
}
