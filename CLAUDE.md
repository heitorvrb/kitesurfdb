# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

A cross-platform desktop database client ("kitesurfdb") written in Rust using Dioxus (desktop mode, webview-based). Supports SQLite and Postgres. See `spec.md` for requirements and `plan.md` for the development plan.

## Build & Test Commands

```bash
cargo build                        # build entire workspace
cargo build -p db                  # build just the db crate
cargo test                         # run all tests
cargo test -p db                   # test just the db crate
cargo test -p db test_introspect   # run a single test by name
cargo run -p app                   # launch the desktop application
dx serve --desktop --package app   # launch with hot reloading (RSX/CSS changes)
dx serve --hotpatch --package app  # launch with experimental Rust-level hot reloading
```

System dependencies required for `app` crate (Dioxus desktop/webview):
```bash
sudo apt-get install -y libwebkit2gtk-4.1-dev libjavascriptcoregtk-4.1-dev libsoup-3.0-dev libgtk-3-dev
```

## Architecture

Cargo workspace with three crates:

- **`crates/db`** — Database abstraction layer (no UI dependency). Defines the `DbBackend` async trait (`traits.rs`) with SQLite (`sqlite.rs`) and Postgres (`postgres.rs`) implementations. All DB types (`QueryResult`, `DbValue`, `SchemaInfo`, `ConnectionConfig`) live in `types.rs`. Uses `sqlx`.
- **`crates/core`** — Business logic bridging `db` and the UI. Package name is `core`, aliased as `app-core` in the `app` crate's Cargo.toml (`use app_core::...`). Contains `ConnectionManager` (save/load/connect), `TabManager` (tab lifecycle), and config persistence (`config.rs`).
- **`crates/app`** — Dioxus desktop binary. `app.rs` is the root component. Components live flat in `src/components/` (not subdirectories). CSS modules per component in `assets/styles/`.

### Config and persistence

- Config stored at `~/.config/kitesurfdb/` — `connections.json` and `preferences.json`
- `ConnectionConfig.password` is `#[serde(skip)]` — passwords are stored/retrieved via OS keyring (`keyring` crate) using the connection UUID as the keyring key
- New optional fields on `ConnectionConfig` must use `#[serde(default)]` for backwards compatibility with existing config files

### Key design decisions

- **Enum-based tabs** (`TabType` enum in `core/tab_manager.rs`), not trait objects — idiomatic for Dioxus component matching. Variants: `SqlEditor`, `TableBrowser`, `TriggerView`, `FunctionView`
- **`CancellationToken`** per tab for aborting in-flight queries on tab close
- **Theming** via CSS custom properties (`--bg-primary`, `--text-primary`, `--border`, etc.) toggled by `data-theme` attribute on the root div; `Theme` enum lives in `core/config.rs`
- **CSS modules** via `#[css_module("/assets/styles/foo.css")]` — class names are mangled/scoped; always reference them as `Styles::class_name`, never as string literals
- **SQL editor** uses `<textarea>` with `syntect` for syntax highlighting rendered as HTML spans — no JavaScript
- **Adding a new DB backend** = implement `DbBackend` trait + add `BackendType` variant + wire in `ConnectionManager::create_backend()`
- **TDD** — always write tests before implementation code

### SQLite vs Postgres schema handling

These two backends differ in introspection output and the sidebar renders them differently:

- **SQLite**: `DbObject.schema` is always `None`; `SchemaInfo.schemas` is empty. Sidebar shows a flat tree (object types as root nodes).
- **Postgres**: `DbObject.schema` is always `Some(...)`. `SchemaInfo.schemas` contains all schema names (including empty ones, populated by a separate `pg_namespace` query). Sidebar shows schemas as root nodes with object types nested inside.

The sidebar detects which mode to use via `has_any_schema(info: &SchemaInfo)`.

### Signal borrow rules (Dioxus)

Never hold a `Signal` borrow (`.read()` / `.write()`) across an `.await` point — this causes a runtime panic. The pattern in async `spawn` blocks is: acquire borrow → extract owned value → drop borrow → `await`.
