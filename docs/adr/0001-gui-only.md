# ADR 0001: GUI-Only Product Surface

## Status

Accepted

## Context

The project started with CLI-shaped leftovers and internal naming that implied a dual CLI/desktop app, but actual use and packaging are desktop-only.

## Decision

The public product surface is GUI-only.

That means:

- no supported CLI workflow
- documentation should describe the desktop app first
- internal naming should avoid `commands` or other CLI-oriented framing when the code is app-layer logic

## Consequences

- README and release docs stay focused on install/run/use as a desktop application
- app-facing internal operations live behind GUI-neutral names such as `app_ops`
- dead or misleading CLI-facing paths should be removed rather than kept as half-supported escape hatches
