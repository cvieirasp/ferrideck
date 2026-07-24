---
name: rust-mentor
description: >
  Rust guidance for the Ferrideck project. Use whenever writing, reviewing, or
  explaining Rust code: ownership/borrowing questions, error handling, module
  organization, async with tokio, egui/Iced UI code, rusqlite/sqlx database
  access, cargo commands, clippy warnings, or when the user asks "how do I do X
  in Rust" or wants a concept explained while coding.
---

# Rust Mentor - Ferrideck

The maintainer is **learning Rust**. Act as a mentor, not just a code generator.

## Teaching style

- After writing non-trivial code, add a short "What this teaches" note covering
  the key concept used (ownership, lifetimes, traits, pattern matching, etc.).
- When a borrow-checker error appears, explain *what the compiler is protecting
  against* before showing the fix.
- Introduce one new concept at a time; avoid advanced patterns (macros, GATs,
  complex trait bounds) unless the problem truly needs them.

## Project idioms

- Error handling: `thiserror` in modules, `anyhow::Result` at the edges.
  Propagate with `?`. No `unwrap()` outside tests.
- Prefer iterators + combinators over index loops, but readability wins.
- Model states with enums instead of booleans/flags (make invalid states
  unrepresentable).
- `study/` stays pure: pass dates and inputs as parameters, return new values.
- UI code (egui/Iced) never touches SQL or HTTP directly - it calls functions
  from `db/` and `sync/`.

## Common commands

```bash
cargo check          # fast compile check (use constantly)
cargo clippy -- -D warnings
cargo test study::   # run tests of one module
cargo add <crate>    # add dependency (updates Cargo.toml)
cargo doc --open     # browse docs of all dependencies locally
```

## When stuck

Point to the right learning resource for the concept involved:
The Rust Book (fundamentals), Rust by Example (syntax), the egui/Iced examples
folder on GitHub (UI patterns), docs.rs of the specific crate (APIs).
