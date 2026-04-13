# AGENTS.md

## Fast Commands
- Build workspace: `cargo build`
- Run all tests: `cargo test`
- Run one crate: `cargo test -p core`, `cargo test -p db`
- Run one test by name: `cargo test -p core test_cycle_order_by`
- Launch desktop app: `cargo run -p app`
- Dioxus hot reload: `dx serve --desktop --package app` (RSX/CSS), `dx serve --hotpatch --package app` (experimental Rust hot reload)

## Required System Packages (Linux, app crate)
- `sudo apt-get install -y libwebkit2gtk-4.1-dev libjavascriptcoregtk-4.1-dev libsoup-3.0-dev libgtk-3-dev libxdo-dev`

## Workspace Map (important boundaries)
- `crates/db`: DB abstraction and backends; `DbBackend` trait in `crates/db/src/traits.rs`
- `crates/core`: app logic and persistence; package name is `core`, imported by app as `app-core`
  - `crates/core/src/tab_manager.rs`: `TabType` enum, `TabManager`
  - `crates/core/src/sql_ordering.rs`: ORDER BY SQL manipulation (`cycle_order_by`, `parse_order_items`, `ColumnOrderInfo`, `SortDirection`)
- `crates/app`: Dioxus desktop UI; root component in `crates/app/src/app.rs`, process entry in `crates/app/src/main.rs`
- CSS modules per component live in `crates/app/assets/styles/`

## Conventions That Matter
- Tabs are enum-driven (`TabType` in `crates/core/src/tab_manager.rs`), not trait objects.
- Tab close must cancel in-flight work via `CancellationToken` (already wired in `TabManager`).
- Dioxus async rule: never hold `Signal` `.read()`/`.write()` borrows across `await`. Pattern: acquire borrow → extract owned value → drop borrow → `await`.
- UI styling uses CSS modules (`#[css_module(...)]` + `Styles::...`), not raw class-name strings.
- When adding optional fields to persisted structs like `ConnectionConfig`, use `#[serde(default)]` for backward compatibility.
- SQL ORDER BY logic lives in `crates/core/src/sql_ordering.rs`; tests for it go in that file's `#[cfg(test)] mod tests`.
- `app_core::tab_manager` re-exports `ColumnOrderInfo` and `SortDirection` from `sql_ordering`.

## Persistence and Secrets
- App config dir: `~/.config/kitesurfdb/`
- Connection metadata: `connections.json`; UI prefs: `preferences.json`
- `ConnectionConfig.password` is intentionally `#[serde(skip)]`; passwords live in OS keyring under service `kitesurfdb` keyed by connection UUID.

## Backend-Specific Schema Behavior
- SQLite introspection is flat: `SchemaInfo.schemas` is empty, `DbObject.schema` is `None`.
- Postgres introspection is schema-aware: `DbObject.schema` is `Some(...)` and `SchemaInfo.schemas` is populated from `pg_namespace`.

## Test Quirks
- Postgres integration tests in `crates/db/src/postgres.rs` are `#[ignore]`; run explicitly with `cargo test -p db -- --ignored`.
- These ignored tests expect a reachable Postgres instance (defaults to `localhost:5432/testdb`, user `postgres`) unless a saved Postgres connection exists in `~/.config/kitesurfdb/connections.json`.

## Existing Repo Instruction Source
- `CLAUDE.md` contains additional project-specific guidance; keep it consistent with this file when updating agent instructions.
