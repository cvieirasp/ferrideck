# 0001 - GUI framework

## Status

Accepted (2026-07-24)

## Context

Ferrideck is a solo project whose primary goal is learning the Rust language itself; shipping the app is secondary. The UI is a desktop flashcard app with simple screens: a deck list, a study session showing one card at a time with a reveal step and four grade buttons, and a card editor. There is no complex layout, no custom rendering and no multi-window requirement.

The framework choice therefore optimizes for one thing: how much of the time spent in front of the editor is spent writing plain Rust, as opposed to learning a framework's own vocabulary.

## Options considered

**egui** - immediate mode. The UI is described by ordinary Rust code that runs every frame, so state lives in plain structs and control flow is `if` and `for`. Minimal ceremony, no message enum, no separate view type. Large ecosystem of examples to learn from.
**Iced** - retained mode following the Elm architecture (model, message, update, view). More structure, which pays off in large applications, at the cost of more boilerplate and a second mental model to learn on top of Rust itself.
**Tauri** - Rust backend with a web frontend. Rejected earlier: the UI would be written in HTML, CSS and TypeScript, which directly contradicts the goal of practicing Rust.

## Decision

Use **egui** through **eframe**, its application framework.

The reason is the learning goal, not technical superiority: egui puts the least framework-specific machinery between an idea and working Rust code. Iced's architecture is valuable, but learning it at the same time as ownership, lifetimes and traits means splitting attention between two unfamiliar things.

## Consequences

- Immediate mode redraws the entire UI every frame, so UI code must stay cheap. Expensive work (SQL queries, sync requests) cannot sit inside a draw call and has to be done outside it, with the result cached in application state.
- egui provides fewer built-in architectural patterns than Iced. Nothing in the framework prevents business logic from leaking into widget code, so the module discipline in `CLAUDE.md` (`ui/` calls `study/`, `db/` and `sync/`, never the reverse) carries more weight here than it would with Iced.
- egui draws its own widgets instead of using native controls, so the app will not look like a platform-native application. Acceptable for this use case.
- Migrating later stays realistic. Because scheduling, persistence and sync live outside `ui/`, replacing the front end (including with Tauri) means rewriting one module, not the application.
- Learning the immediate mode model is itself transferable: it is the same idea behind Dear ImGui, widely used in game and tooling development.
