# Reliability

This app is local-first, network-dependent, and GUI-driven, so reliability work focuses on graceful failure, state recovery, and minimizing surprise.

## Current safeguards

### Panic-safe background tasks

`src/gui/tasks.rs` wraps blocking worker tasks with `catch_unwind`.

If a worker panics:

- the GUI stays alive
- the task failure is classified as an internal error
- Diagnostics keeps a recent failure record
- retry actions avoid enqueuing nonsense jobs when there is no retryable payload

### Settings recovery

If `settings.json` is invalid:

- startup reports the parse failure
- the invalid file is renamed to `settings.invalid-<timestamp>.json`
- the app continues with default settings

This preserves the broken file for inspection instead of silently discarding it.

### Duplicate import protection

Import flow protects against duplicates in two places:

- repeated selected URLs in the same batch
- duplicate article content using `content_fingerprint`

That keeps retries and multi-section selection flows from creating repeated local entries.

### SQLite durability defaults

The app uses:

- `journal_mode = WAL`
- `synchronous = NORMAL`
- `foreign_keys = ON`
- `busy_timeout = 5s`

Those settings balance responsiveness with durable local persistence for a single-user desktop app.

### One-time backfills

Maintenance backfills run at open time for:

- preview summaries
- content fingerprints
- generated topics
- duplicate `clean_text` compaction

Each backfill is guarded by an `app_state` flag so it runs once per database.

## Diagnostics posture

The Diagnostics screen can:

- open the data folder
- open/copy the log
- build a support bundle
- clear the browse cache
- compact the DB
- rebuild FTS
- run SQLite integrity check

Performance counters currently expose:

- browse cache hit/miss counts
- average library page query time
- average content refresh time

## Known tradeoffs

- the shared DB handle is still a mutex-guarded database facade, not a dedicated DB worker
- queue payload tables are still JSON blobs rather than normalized relational tables
- some library rendering still has an in-memory fallback path when no page cache is ready

Those are reasonable local-first tradeoffs today, but they are good candidates for future hardening if the app grows.
