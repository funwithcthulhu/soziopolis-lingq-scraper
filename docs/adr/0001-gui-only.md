# ADR 0001: GUI-Only Product Surface

## Status

Accepted

## Context

The repository used to have some CLI-shaped leftovers and names that suggested a dual CLI/desktop app. In practice, the product is packaged, tested, and used as a desktop app only.

## Decision

The supported product surface is GUI-only.

That means:

- there is no supported CLI workflow
- documentation should describe the desktop app first
- internal naming should avoid `commands` or other CLI framing when the code is really app-layer logic

## Consequences

- README and release docs stay focused on install, run, and use as a desktop application
- app-facing internal operations live behind GUI-neutral names such as `app_ops`
- dead or misleading CLI-facing paths should be removed rather than kept as half-supported escape hatches
