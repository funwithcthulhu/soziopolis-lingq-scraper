# Data Model

The tool stores its working state in a local SQLite database.

## Core table: `articles`

Each row represents one saved Soziopolis article.

Important columns:

- `id`
  - local primary key
- `url`
  - canonical source URL, unique
- `title`, `subtitle`, `teaser`
  - listing and display metadata
- `preview_summary`
  - compact preview text used by the library UI and search
- `author`, `date`, `published_at`, `section`
  - article metadata
- `generated_topic`
  - derived topic label computed from article fields
- `custom_topic`
  - user override topic
- `source_kind`, `source_label`
  - provenance metadata for browse/import flows
- `content_fingerprint`
  - duplicate-content guard
- `body_text`
  - raw stored body text
- `clean_text`
  - import/export oriented cleaned text
- `word_count`, `fetched_at`
  - sizing and freshness metadata
- `uploaded_to_lingq`, `lingq_lesson_id`, `lingq_lesson_url`
  - LingQ upload state

## Derived topic behavior

Effective topic is resolved as:

`COALESCE(NULLIF(custom_topic, ''), generated_topic)`

That expression is used in:

- library filtering
- library grouping and ordering
- topic counts and diagnostics

## Full-text search

`articles_fts` is an FTS5 virtual table over article search fields.

Indexed search fields include:

- `title`
- `subtitle`
- `teaser`
- `preview_summary`
- `author`
- `section`
- `custom_topic`
- `generated_topic`
- `body_text`
- `url`

SQLite triggers keep the FTS table synchronized with `articles`.

## App state and queue persistence

The app also stores lightweight process state in SQLite:

- `app_state`
  - scalar settings such as queue metadata and one-time backfill markers
- `job_queue`
  - queued import/upload jobs
- `completed_jobs`
  - recent finished jobs
- `failed_fetches`
  - retryable import failures
- `failed_uploads`
  - retryable upload failures
- `job_history`
  - append-oriented summary history

The queue payload tables currently store JSON payloads. That keeps the schema simpler while the job model remains small and app-local.

## Indexes

Important indexes cover:

- `section`
- upload state
- `word_count`
- `published_at`
- `content_fingerprint`
- generated/effective topic
- case-insensitive title sort

These indexes cover the library's main filter and sort paths without moving away from SQLite.
