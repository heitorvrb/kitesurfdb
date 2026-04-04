# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

A cross-platform desktop database client written in Rust using Dioxus (desktop mode, webview-based). Supports SQLite now, Postgres planned. See `spec.md` for requirements and `plan.md` for the development plan.

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

- **`crates/db`** — Database abstraction layer (no UI dependency). Defines the `DbBackend` async trait (`traits.rs`) with SQLite implementation (`sqlite.rs`). All DB types (`QueryResult`, `DbValue`, `SchemaInfo`, `ConnectionConfig`) live in `types.rs`. Uses `sqlx`.
- **`crates/core`** — Business logic bridging `db` and the UI. Contains `ConnectionManager`, `TabManager`, and config persistence (currently scaffolded).
- **`crates/app`** — Dioxus desktop binary. `app.rs` is the root component with connection bar, SQL editor textarea, and result table. CSS styles in `crates/assets/styles/main.css`.

Could have, in the future, components organized under `src/components/` with subdirectories: `sidebar/` (connection list, schema tree), `tabs/` (tab bar, SQL editor, table browser), `shared/` (result grid, modals, status bar). Global state via Dioxus Signals in `state.rs`.


### Key design decisions

- **Enum-based tabs** (`TabType` enum), not trait objects — idiomatic for Dioxus component matching
- **`CancellationToken`** per tab for aborting in-flight queries on tab close
- **Passwords** stored via OS keyring (`keyring` crate), never in plaintext config files
- **Theming** via CSS custom properties toggled by `data-theme` attribute (light/dark)
- **SQL editor** uses `<textarea>` with `syntect` for syntax highlighting — no JavaScript dependencies
- **Adding a new DB backend** = implement `DbBackend` trait + add `BackendType` variant
- **TDD** — always write tests before implementation code
