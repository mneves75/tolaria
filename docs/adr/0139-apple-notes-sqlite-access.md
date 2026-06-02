# ADR-0139: Apple Notes SQLite Access (WAL-Safe Copy + Read)

## Status

Accepted

## Context

Apple Notes stores its data in `NoteStore.sqlite` under the user's Group Container. Reading it has two hazards the importer must handle:

1. **The Notes app holds the database open in WAL mode.** Recently edited notes can live only in the `-wal` write-ahead log, not the main `.sqlite` file. Copying the main file alone, or opening it read-only, silently loses the user's most recent notes — the worst possible failure for a migration.
2. **The schema is large and undocumented.** The note↔body relationship and the relevant columns are reverse-engineered.

## Decision

Add `rusqlite` (feature `bundled`, so the app ships a known SQLite version rather than depending on the host library) and a `import::store` module:

- **`copy_database`** copies `NoteStore.sqlite` **and its `-wal` and `-shm` sidecars** to a throwaway working directory. The sidecars are load-bearing; copying only the main file would miss WAL-resident notes.
- **`open_checkpointed`** opens the *copy* **read-write** (never the user's live database) and runs `PRAGMA wal_checkpoint(TRUNCATE)`. A read-only connection cannot replay the WAL, so the latest notes would be invisible; opening the disposable copy read-write lets SQLite merge the WAL. This is the corrected approach from the design review.
- **`enumerate_notes`** runs a research-derived query joining `ZICNOTEDATA` to `ZICCLOUDSYNCINGOBJECT` on `ZNOTE = Z_PK`, returning `RawNote { source_id (ZIDENTIFIER), title (ZTITLE1), created (ZCREATIONDATE), modified (ZMODIFIEDDATE1), password_protected (ZISPASSWORDPROTECTED), body_gzip (ZDATA) }`.

## Consequences

- The WAL-integrity guarantee is unit-tested: a note written only to the WAL (autocheckpoint off, writer connection still open) is recovered after copy + read-write open. A read-only open would fail that test.
- End-to-end reading is tested against an in-test SQLite built with the same column names, decoding a real gzipped-protobuf blob to markdown — no real Apple data or `protoc` required.
- **The schema is research-derived (threeplanetssoftware / Obsidian), not validated against live databases.** The column names and the note↔body join must be confirmed against real `NoteStore.sqlite` files across macOS versions (the design's corpus matrix). Version-specific concerns deferred to that validation: runtime discovery of the `Z_*NOTES` join-table index, filtering `ZMARKEDFORDELETION` / Recently Deleted, account/folder columns.
- `rusqlite` `bundled` adds a C-compiled SQLite to the build (build time + binary size); acceptable for a first-class import feature.
