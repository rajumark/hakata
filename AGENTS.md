# Hakata development guidance

## Build & verification

- Always verify with `cargo build`, `cargo test`, and `cargo clippy --all-targets` before declaring a change done.
- Keep the build warning-free; fix any new warning before moving on.
- No GUI/visual verification is possible in this environment (screencapture is blocked); compile + tests are the source of truth.
- Do not commit unless the user explicitly asks.

## Structure

- `src/app.rs` is split into modules under `src/app/`: `mod.rs` (struct, layout, page dispatch), `sidebar.rs`, `apps.rs`, `debug.rs`, `settings.rs`.
- Keep per-page logic in its matching module; do not grow `mod.rs`.
- `Hakata` methods used across modules must be `pub(crate)`.

## GPUI (fork) quirks

- The gpui fork has no built-in text field; use `src/input.rs` (`SearchField` + `EntityInputHandler`).
- `overflow_y_scroll` / `overflow_hidden` etc. are `StatefulInteractiveElement` methods: call `.id(...)` first.
- There is no `with_style` on `Div`; use the style builder methods (`left`, `right`, ...) directly.
- `SharedString` does not implement `AsRef<OsStr>`: call `.as_str()` before passing to `Command` args.
- `EntityInputHandler` trait methods take `&mut Window`.

## Patterns

- UI strings go through `tr!`/`tr_cow!` (rust-i18n) with keys in `locales/{app,ja,zh-CN}.yml`; add keys to all three catalogs. Keep pure parsers/builders English-only so their tests stay deterministic; translate at render.
- Long-lived background work goes to `cx.background_executor().spawn`; store the result on the entity, `cx.notify()`, and guard with a generation counter so a superseded result cannot overwrite newer state.
- Render reads only cached state; never do I/O from `render`.
- Do not add comments unless asked; match the existing style.
- Use Waku (`../learning`) as the UX reference; use the forked gpui checkout (`~/.cargo/git/checkouts/zed-4d64e9894aeee3ad/5415508`) as the GPUI API reference.
