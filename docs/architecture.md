# Architecture

This repo is a small Windows desktop tool written in Rust. `iced` handles the GUI, SQLite stores local data, and blocking worker tasks handle scraping, imports, refreshes, and LingQ uploads.

## Code layout

- `src/main.rs`
  - starts logging and launches the GUI
- `src/gui.rs`
  - app shell and active GUI module tree
- `src/gui/`
  - `state.rs`: long-lived application state
  - `message.rs`: GUI messages
  - `update.rs`: state transitions and command orchestration
  - `views.rs`: UI rendering
  - `tasks.rs`: background task wrappers and panic-safe worker execution
  - `helpers.rs`: shared formatting, topic, and small utility helpers
- `src/services.rs`
  - application-service layer for browsing, importing, library refresh, and LingQ upload flows
- `src/database.rs`
  - database facade and query methods
- `src/database/`
  - `migrations.rs`: schema and index evolution
  - `maintenance.rs`: one-time backfills and storage maintenance
  - `types.rs`: row mapping and article-derived helpers
- `src/soziopolis.rs`
  - site scraping, browse cache, and article parsing
- `src/lingq.rs`
  - LingQ HTTP client
- `src/app_ops.rs`
  - small app-facing operations used by the GUI

## Data flow

1. The GUI triggers a user action through a `Message`.
2. `src/gui/update.rs` mutates local state and, when needed, spawns a blocking task through `src/gui/tasks.rs`.
3. The task calls an application service or app op.
4. Services use repositories/database methods plus external clients (`soziopolis`, `lingq`).
5. Results return to the GUI through typed messages and are applied back into state.

## Module boundaries

- GUI modules own presentation state and user interaction.
- Services own cross-step workflows such as import, upload, and refresh.
- Database code owns schema, filtering, paging, and persistence concerns.
- Scraper and LingQ clients should stay free of GUI state and storage policy.

## Library query model

The library flow now uses typed query structs from `src/domain.rs`:

- `LibraryQuery`
  - search text
  - optional topic filter
  - upload status filter
  - min/max word count
- `LibraryPageRequest`
  - sort mode
  - optional topic grouping
  - paging offset/limit

These structs keep the query shape consistent from the GUI down to the database.

## Topic model

Each article has:

- `custom_topic`
  - user-controlled override stored in SQLite
- `generated_topic`
  - deterministic topic derived from title, subtitle, section, and URL

The effective topic is:

`custom_topic` if present, otherwise `generated_topic`

That effective topic is used for filtering, ordering, and diagnostics.

## Background work model

The app uses blocking worker tasks for:

- browse refresh
- article preview fetch
- import
- upload
- content refresh

`src/gui/tasks.rs` wraps those tasks with panic capture so worker crashes become `AppError::Internal` instead of taking the GUI down.

## Likely future cleanup

If this tool keeps growing, the next cleanup steps are:

- splitting `src/services.rs` into browse/import/library/LingQ submodules
- splitting `src/database.rs` into article-query and job-persistence submodules
- moving more library display queries to DB-backed page refreshes instead of in-memory fallbacks
