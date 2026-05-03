# ADR 0003: Blocking Worker Tasks with Panic Capture

## Status

Accepted

## Context

The app performs network scraping, import, refresh, and upload work that should not block the GUI thread. At the same time, the codebase is intentionally lightweight and does not need a larger async runtime model everywhere.

## Decision

Use blocking worker threads for heavy tasks and route completion back into the GUI through typed messages.

Wrap worker execution with panic capture so internal crashes become structured task failures.

## Consequences

- the GUI remains responsive during browse/import/upload flows
- failures surface through `AppError` instead of application crashes
- diagnostics can retain recent internal-task failures
- task code should stay explicit about progress, cancellation, and retryability
