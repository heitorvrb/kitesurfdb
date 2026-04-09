# DB Client — Development Plan

## Context

Building a cross-platform desktop database client in Rust using Dioxus. The goal is a lightweight, performant app where users can save DB connections, browse schema objects, and query databases through a tabbed interface with an SQL editor.

---

## Technology Stack

| Component | Choice | Rationale |
|-----------|--------|-----------|
| UI Framework | **Dioxus 0.7+ desktop** | Cross-platform via webview, CSS theming, async-friendly |
| Database | **sqlx 0.8** | Unified async API for both Postgres and SQLite |
| SQL Editor | **Plain `<textarea>` + `syntect`** | Syntax highlighting via pure Rust |
| Async Runtime | **tokio** | Used by both Dioxus and sqlx |
| Config | **serde + serde_json** | Save connections/preferences to `~/.config/kitesurfdb/` |
| Other | `dirs`, `uuid`, `chrono`, `thiserror`, `tracing`, `tokio-util` (CancellationToken), `syntect`, `keyring` |

---

## Project Structure

```
kitesurfdb/
  Cargo.toml                  # workspace root
  spec.md
  assets/
    styles/                   # CSS themes (light.css, dark.css)
    icons/
  crates/
    app/                      # binary — Dioxus desktop UI
      src/
        main.rs
        app.rs                # root component
        state.rs              # global signals (ConnectionManager, TabManager, Theme)
        theme.rs
        components/
          sidebar/            # connection list, schema object tree
          tabs/               # tab_bar, tab_container, sql_editor, table_browser
          shared/             # result_grid, sql_display, modal, status_bar
    db/                       # library — database abstraction (no UI dependency)
      src/
        traits.rs             # DbBackend trait
        types.rs              # QueryResult, DbValue, SchemaInfo, ConnectionConfig
        error.rs
        postgres.rs
        sqlite.rs
        introspection.rs
    core/                     # library — business logic bridging db and UI
      src/
        connection_manager.rs # save/load/connect/disconnect
        tab_manager.rs        # tab lifecycle, resource cleanup
        config.rs             # persistence to disk
        query_executor.rs     # async query execution with cancellation
```

---

## Core Architecture

### Database Abstraction (`db` crate)

A `DbBackend` async trait with implementations for Postgres and SQLite:
- `connect()`, `disconnect()`, `execute_query(sql)`, `introspect()`
- Adding a new backend = new module + implement trait + add `BackendType` variant
- `QueryResult` holds columns, rows (`Vec<Vec<DbValue>>`), rows affected, execution time, and the SQL string
- `introspect()` returns `SchemaInfo` (tables, views, triggers, functions) using backend-specific queries (`information_schema` / `sqlite_master`)

### Connection Manager (`core` crate)

- Stores saved `ConnectionConfig` list (persisted to JSON) and active `Box<dyn DbBackend>` handles
- Lives behind a `Signal<ConnectionManager>` in Dioxus for reactive UI updates
- Passwords stored securely via OS keyring (`keyring` crate) from the start

### Tab System (`core` crate)

Enum-based (idiomatic for Dioxus component matching):

```rust
enum TabType {
    SqlEditor { sql_content, result },
    TableBrowser { object_name, generated_sql, result, page },
    TriggerView { trigger_name, definition },
}
```

- Each `Tab` has a `CancellationToken` — on close, in-flight queries are cancelled and all data is dropped
- `TabManager` signal is the single source of truth for all tab state
- Tab container component matches on `TabType` to render the correct view

### Query Data Flow

```
User action (Run button / Ctrl+Enter / click table in sidebar)
  -> spawn async task with CancellationToken
  -> look up active DbBackend by connection_id
  -> db_backend.execute_query(sql).await
  -> check cancellation
  -> write QueryResult into tab's state via TabManager signal
  -> Dioxus reactively re-renders result_grid
```

### Theme System

- CSS custom properties (`--bg-primary`, etc.) toggled via `data-theme` attribute on root div
- `Signal<Theme>` in app state, persisted to config

---

## Phased Development

### Phase 1: Foundation ✅
**Goal:** A window that connects to SQLite and runs queries.

- [x] Initialize Cargo workspace with `app`, `db`, `core` crates
- [x] Define `DbBackend` trait and core types (`QueryResult`, `DbValue`, `ConnectionConfig`)
- [x] Implement SQLite backend
- [x] Basic Dioxus desktop shell: single pane with `<textarea>` for SQL, Run button, HTML `<table>` for results
- [x] Wire up async query execution end-to-end

### Phase 2: Tab System + Table Browser ✅
**Goal:** Multiple tabs, sidebar with schema tree, click-to-browse.

- [x] Implement `TabManager` (open/close/switch tabs)
- [x] Build `tab_bar` and `tab_container` components
- [x] Implement `introspect()` for SQLite
- [x] Build sidebar with collapsible object tree
- [x] `TableBrowser` tab type (click table -> `SELECT * FROM table LIMIT 100`)
- [x] SQL display strip at top of every tab
- [x] CancellationToken for in-flight queries on tab close

### Phase 3: Postgres + Connection Management ✅
**Goal:** Multi-database support with saved connections.

- [x] Implement Postgres `DbBackend` (sqlx postgres feature)
- [x] Implement `introspect()` for Postgres (information_schema)
- [x] Save/load connections to JSON config file
- [x] Connection dialog modal (add/edit/delete)
- [x] Sidebar: saved connections list with connect/disconnect

### Phase 4: SQL Editor Polish + Theming ✅
**Goal:** Better editor UX, light/dark mode.

- [x] Ctrl+Enter keyboard shortcut to execute query
- [x] Light/dark theme CSS with toggle, persisted preference
- [x] Add views, triggers, functions to sidebar tree + corresponding tab types
- [x] SQL syntax highlighting via `syntect` (render highlighted HTML spans alongside the textarea)

### Phase 5: Robustness + UX
**Goal:** Production-quality error handling and polish.

- [x] Change sidebar structure: instead of tables with schemas inside, views with schemas inside, etc, have schemas as root items and then tables, views, etc. inside
- [x] Error display (connection failures, query errors)
- [x] Pagination in table browser
- [x] Result row limit (default 100) with "load more"

### Phase 6: Quality of life

- [x] Together with Rows: 100 | Time: 17.17839ms, show a count of total items. So it will now make two queries, one for counting and one for limit 100. If there are less than 100 rows, no need to show the second count.
- [x] Together with Rows: 100 | Time: 17.17839ms, show a refresh button. Clicking it should re-do the query(ies).
- [x] When a table has no lines, show the header with its columns anyway.
- [x] Add a button to show/hide the sidebar, and make it permanent by saving its state to the configuration.json file.
- [ ] F5 should refresh the results of the opened tab
- [ ] Middle click on the tab handle should close the tab



### Future (Phase 6+)
- Query history, CSV export, EXPLAIN visualization, MySQL backend, auto-complete from schema, table DDL viewer

---

## Verification

After each phase, verify by:
1. `cargo build` — workspace compiles cleanly
2. `cargo test -p db` — database abstraction tests pass (use a test SQLite DB; Postgres tests need a running instance)
3. `cargo run -p app` — launch the desktop app and manually test the features added in that phase
4. For Phase 3+: test both SQLite and Postgres connections end-to-end
