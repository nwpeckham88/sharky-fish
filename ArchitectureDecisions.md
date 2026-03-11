# Architecture Decisions

## Intake Context Persistence

Decision: use a hybrid model.

- SQLite remains the runtime source of truth for queue state, intake filtering, search, and UI queries.
- A hidden sidecar JSON file persists durable per-item context next to managed media so a reset or full rescan can restore that context.
- Media container metadata should not be used for sharky-fish state.

### Why this is the right tradeoff

- Database only is fast and operationally clean, but it loses context after a reset unless the database is preserved and restored.
- Embedded media metadata is format-specific, brittle across remux/transcode workflows, and hard to inspect or repair in bulk.
- Sidecars are portable, human-inspectable, and move with the library. That matters if the goal is to shape up an existing library over time without losing prior decisions.
- The app already needs a database anyway for queueing, SSE-driven UI state, and efficient batch operations, so using sidecars alone would make normal operation slower and more awkward.

### What gets stored where

SQLite stores volatile and query-heavy state:

- intake status
- queue and task lifecycle
- current library index
- cached probe data
- derived search/filter fields

Sidecar files store durable library context:

- sharky-fish item id
- source fingerprint fields needed for re-association
- selected internet metadata ids and title/year
- organization decision and target placement intent
- last accepted processing decision summary
- managed status such as unprocessed, reviewed, approved, processed, or kept-original
- timestamps for first seen and last updated

Sidecars should not store transient execution details like live progress, temporary failures, or active task state.

### Sidecar shape

Use a hidden JSON sidecar adjacent to the media item, named with the same stem.

Examples:

- `Movie (2024).mkv` -> `Movie (2024).sharky.json`
- `Episode S01E02.mp4` -> `Episode S01E02.sharky.json`

This fits the current organizer behavior because stem-based renames already preserve matching sidecars during movie-folder reshaping.

### Intake behavior after this decision

- Library rescan imports any discovered sidecar context into SQLite.
- Files with sidecar context reappear with their prior sharky-fish decisions intact.
- Files without sidecar context are treated as unprocessed and should be surfaced in Intake for triage.
- Intake becomes the place for both newly ingested files and existing library items that have never been reviewed by sharky-fish.

### Implementation boundary

Do this next:

1. Add a durable `managed_items` table in SQLite for the normalized runtime model.
2. Add sidecar read/write helpers and import them during library rescan and library watcher updates.
3. Populate Intake from `managed_items`, not only from ingest-created jobs.
4. Write sidecars only when a user or accepted AI decision changes durable context.

Do not do this:

- write sharky-fish state into media container tags
- treat sidecars as the live queue engine
- store noisy execution logs in sidecars