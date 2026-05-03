# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Changed

- Simplified the project to a GUI-only desktop workflow and removed stale CLI-facing docs and internal naming.
- Added explicit architecture, data-model, reliability, and ADR documentation for the desktop app.
- Added typed library query/page request models and DB-backed generated topics for cleaner filtering and sorting paths.

### Fixed

- Hardened background task execution so worker-thread panics are surfaced as internal app errors instead of crashing the GUI flow.
- Improved settings and import reliability around invalid settings files, duplicate selections, and machine-specific path fallback.
- Invalid settings files are now quarantined as `settings.invalid-<timestamp>.json` instead of only failing to parse.
- Library diagnostics now track average page-query and content-refresh timings.

## [1.1.0] - 2026-05-02

Initial public release of Soziopolis Reader.

### Highlights

- Windows installer packaging with Inno Setup
- Portable Windows bundle workflow
- Desktop browsing, local library management, and LingQ upload flow
- Persisted import and upload queue state in SQLite
- Diagnostics tools for logs, support bundles, retry lists, and database maintenance
