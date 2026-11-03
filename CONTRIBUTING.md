# Contributing to CleanShini

Thanks for your interest. CleanShini is a Rust workspace.

## Build

```bash
cargo build --release
```

Binaries: `target/release/cleanshini` (CLI + GUI unified).

## Structure

- `crates/cleanshini-core` — scanning engine, rules, executor, audit, system cleanup.
- `crates/cleanshini` — unified binary (CLI + egui GUI), i18n.

## Adding a language

Edit `crates/cleanshini/src/i18n.rs`: add a new `Lang` variant and a `static` translation table. Wire it into `strings()` and the top-bar toggle.

## Adding a cleaning category

1. Add variant to `Category` in `cleanshini-core/src/model.rs` (as_str, from_str, all()).
2. Add classification in `scanner.rs`.
3. Add color in `cleanshini/src/main.rs::category_color`.
4. Add to filter list and i18n.

## Portability

See README "Portability". Windows-only features (recycle bin, DISM, etc.) are gated with `#[cfg(windows)]`.
```