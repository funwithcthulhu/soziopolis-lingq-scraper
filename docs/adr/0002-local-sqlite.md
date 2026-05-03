# ADR 0002: Local SQLite as the Primary Store

## Status

Accepted

## Context

The app is single-user, local-first, and Windows desktop oriented. It needs structured persistence, text search, portability, and simple support-bundle generation.

## Decision

Use SQLite as the primary application database.

Key configuration:

- WAL mode
- FTS5 for search
- bundled SQLite via `rusqlite`

## Consequences

- installs remain self-contained
- diagnostics and support bundles stay straightforward
- search/indexing can evolve with migrations instead of adding another service
- the codebase should prefer query/index improvements before considering a heavier DB stack
