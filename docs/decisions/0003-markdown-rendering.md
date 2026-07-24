# 0003 - Markdown rendering

## Status

Accepted (2026-07-24)

## Context

Card fronts and backs are stored as Markdown in plain text. That was decided when the data model was written, before anything could render it: the database holds the source text, not a formatted document, so the storage format has never depended on how the app draws it.

M5 is where that text has to start looking like what it says. A language card wants emphasis on the word being learned, bold on a correction, and a short list for the forms of a verb. Until now the review screen has printed the raw source, backticks and asterisks included, which is worse than no formatting at all.

Editing stays syntax-based: the user types Markdown in the card editor and sees the rendered result in a preview, rather than a WYSIWYG editor. This follows from ADR 0001. Immediate mode redraws everything every frame from plain state, and a rich text editor would mean maintaining a formatted document model inside the UI layer, which is precisely the kind of framework-specific machinery that ADR chose egui to avoid.

## Options considered

**egui_commonmark** - a maintained CommonMark renderer for egui. It parses with `pulldown-cmark` and emits egui widgets, so rendered text participates in normal layout and follows the active egui theme. It is versioned in step with egui, which means the compatibility burden is a single number to keep aligned.

**Hand-rolled parsing into a `LayoutJob`** - egui's `LayoutJob` allows per-range text formatting, so emphasis could be applied by scanning the source and pushing styled sections. This gives total control over the result and adds no dependency. The cost is that "scanning the source" is writing a Markdown parser: nested emphasis, escapes, lists, code spans and the ambiguities CommonMark exists to settle. It is a large scope and an easy place to be subtly wrong, for a feature that is not the point of this project.

**Plain text, no rendering** - rejected. It is the current behaviour and the milestone exists to end it. Showing `**word**` to the user gives the syntax cost of Markdown with none of its benefit.

## Decision

Use **egui_commonmark**, pinned to the release that targets the egui version in use (0.24.0 for egui 0.35).

It is added with default features off and only the parser enabled, because image loading is not needed yet and it drags in a decoding stack that would otherwise be compiled for nothing.

Rendering is wrapped in a single function in `ui/markdown.rs`. No other module names the crate.

## Consequences

- The dependency tree grows by a CommonMark parser and its support crates. This is the price of not writing one, and it is a good trade: a parser is a well-defined problem someone else has already solved correctly.
- `egui_commonmark` must track the egui minor version. egui is pre-1.0, so `0.34` and `0.35` are incompatible majors under semver, and mixing them compiles two copies of egui into the binary. The symptom is confusing rather than obvious: type errors that read as `expected &mut egui::Ui, found &mut egui::Ui`. Every egui upgrade therefore has to move both crates together.
- Styling is bounded by what the renderer exposes. It integrates with the egui theme, so text follows the app's fonts and colours, but anything beyond that means configuring the viewer rather than drawing freely. Accepted: card content should look like the rest of the app.
- The renderer needs a cache that survives between frames, which has to be owned by the application struct. This is the same rule immediate mode already imposes on everything else, so it costs a field, not a new pattern.
- Swapping the renderer later stays realistic. What is stored is Markdown source, which no renderer owns, and the only code that knows which crate draws it is `ui/markdown.rs`. Replacing it is a change to one file plus a dependency line.
