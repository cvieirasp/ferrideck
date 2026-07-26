# Issue #42 - Live preview in the card editor

Branch: `feat/editor-live-preview`. Milestone M5 - Markdown Cards.

## Plan

- [x] Read `ui/card_editor.rs`, `ui/markdown.rs` and `ui/review.rs` before touching anything
- [x] Promote `render_markdown_as` from `ui/review.rs` into `ui/markdown.rs` so the
      editor can render each field in the *same* style the review screen uses
      (front heading, back body, example small), not just with the same parser
- [x] Card editor: one input + one preview per field (front, back, example),
      both fed from the same `String`
- [x] Layout: `ui.available_width()` against a single threshold constant,
      `ui.columns(2, ..)` above it, stacked below it
- [x] Empty field: italic weak "preview" hint inside the preview box, never an error
- [x] One-line syntax reminder, drawn as plain text (not through the renderer)
- [x] Whole screen wrapped in a `ScrollArea` (same pattern as #41)
- [x] Save flow from #25 verified untouched: same validation, same status message
- [x] Unit tests for the layout threshold and the hint decision
- [x] `cargo fmt`, `cargo clippy -- -D warnings`, `cargo test` clean

## Follow-up after the first manual run

- [x] Enter had to be a real line break, in the preview and in the review.
      CommonMark renders a lone newline as a space and `egui_commonmark` has no
      option for it, so `ui/markdown.rs` rewrites soft breaks into hard breaks
      before the source reaches the renderer. Recorded in ADR 0003.

## Manual test script

`cargo run`, then:

1. **Deck guard.** Open Card editor with no deck selected. Expect the "Select a
   deck" line and nothing else. Go to Deck list, create or select a deck, come
   back.
2. **Syntax reminder.** The line under the heading reads literally
   `**bold**   *italic*   - list`, in monospace and dimmed. It must *not* be
   rendered: no bold "bold", no bullet.
3. **Live preview, front.** Type into Front, one character at a time:
   `The **cat** sat on the *mat*`. The preview changes on every keystroke, with
   no Save, no Tab, no click. Bold and italic where the markers are, markers
   gone.
4. **Live preview, back.** In Back, type a list:
   ```
   Forms of *to be*:

   - am
   - is
   - are
   ```
   Expect real bullets and italics on "to be", and note that the preview text is
   smaller than the front's, which is the study-time difference between a card
   front and a card back.
5. **Enter is a line break.** In Back, on a single paragraph with no blank line
   between them, type `am`, Enter, `is`, Enter, `are`. The preview must show
   three lines, not `am is are`. Check the same card in Review later (step 11):
   three lines there too. Then check that a blank line still separates
   paragraphs, and that a fenced block typed with ``` keeps its code intact.
6. **Live preview, example.** In Example, type `She **is** at home.` The preview
   renders in the small style, matching how examples appear while studying.
7. **Empty hint.** Select all in Back and delete. The box stays, showing italic
   dimmed `preview`. Type three spaces: still the hint, no error, no red, and
   Save is still disabled.
8. **Layout switch.** With the window at its default 900 points, each field is
   input left, preview right. Drag the window narrower: at around 700 points of
   editor width all three fields flip to stacked, input above preview. Widen it
   again and they flip back. Nothing is lost or reset across the switch.
9. **Scroll.** Make the window short (roughly 400 points tall) while stacked. A
   vertical scrollbar appears; scroll to the bottom and the Save button is
   reachable. The nav bar and the status bar stay put.
10. **Save flow unchanged.** Clear Front: Save is disabled. Type only spaces in
    Front: still disabled. Fill Front and Back: Save enables. Press it. Status
    bar reads `Card "<front>" saved` with the *trimmed* front, and all three
    fields empty, each preview back to its hint.
11. **Preview equals reality.** Go to Review, Start, and study the card just
    saved. The front is the same size and the same emphasis as the editor
    preview showed; reveal the answer and check the back list and the example
    the same way. Only the line breaks may fall differently, because the review
    card box is wider than a half-width preview column.
12. **Shortcuts still work.** During that review, Space reveals and 1-4 rate, as
    in #34: nothing in the editor changed what the review screen claims from the
    keyboard.

## Review

`ui/markdown.rs` gained `render_markdown_as`, moved verbatim from `ui/review.rs`
and switched to taking `&TextStyle` instead of an owned one (it is only read).
This is what makes the preview a real preview: the editor and the review screen
now share the renderer *and* the style mapping, so the only thing that can still
differ between the two is line wrapping, which follows the width of the box.

`ui/card_editor.rs` grew three small functions - `field`, `input`, `preview` -
so the three fields differ only by their arguments. The save block was moved
into the scroll area closure but is otherwise byte-identical to what #25 wrote:
same trimming, same `can_save`, same `Utc::now()` at the edge, same
`db::create_card` call, same `Card "..." saved` status.

`with_hard_line_breaks` in `ui/markdown.rs` turns the newlines the user typed
into CommonMark hard breaks, right before the source is parsed. It sits in
`render_markdown`, which both screens go through, so the editor and the review
screen could not disagree about it even by accident. It returns a `Cow`, so a
one-line card front, the common case, allocates nothing.

Not done here: the previews are half-width when side by side, so a long line
wraps in the editor at a point it would not wrap while studying. That is a
property of any rendered document and not a divergence in rendering. Indented
code blocks (four spaces, no fence) are also not detected by
`with_hard_line_breaks`; the effect is two invisible trailing spaces inside the
code, never a change in structure.
