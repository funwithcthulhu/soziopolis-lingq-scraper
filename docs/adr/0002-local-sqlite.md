# ADR 0002: Local SQLite as the Primary Store

## Status

Accepted

## Context

The app is a single-user Windows desktop tool. It needs structured local persistence, text search, portability, and support bundles that are easy to inspect.

## Decision

Use SQLite as the primary application database.

Key configuration:

- WAL mode
- FTS5 for search
- bundled SQLite via `rusqlite`

## Consequences

- installs stay self-contained
- diagnostics and support bundles stay simple
- search and indexing can evolve through migrations instead of another service
- the codebase should prefer query and index improvements before considering a heavier DB stack
