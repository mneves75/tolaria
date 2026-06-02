# ADR-0140: Apple Notes Import Orchestration and Content Hashing

## Status

Accepted

## Context

The import modules built so far each do one job: read the store, decode bodies, convert formatting, derive filenames, record a manifest. Something has to drive them in order and write notes to disk safely — including on a *re-run*, where the only unforgivable outcome is overwriting a note the user edited in Tolaria after a previous import.

The manifest's re-import policy (ADR-0137) compares a stored content hash against the file's current hash. That requires a stable hash function, which ADR-0137 deferred to "the slice that introduces it."

## Decision

Add `import::run::run_import(notes_dir, dest_dir, prior_manifest)`, a Tauri-free orchestration that takes plain directories so it is fully unit-testable:

1. Copy the database (WAL-safe, ADR-0139), open, enumerate notes ordered by `Z_PK` for deterministic filename assignment across runs.
2. For each note: skip + report locked notes; assemble content; choose a destination path (a previously-imported note keeps its recorded path; a new note gets a fresh slug, seeded so it cannot collide with an already-placed note); then apply the manifest's re-import decision:
   - **Create / Update** → write via the existing `vault::save_note_content`, set the file's modified time to the note's Apple date (Tolaria sources dates from the filesystem), and record a manifest entry.
   - **PreserveUserEdit** → leave the file untouched, report it, and carry the prior manifest entry forward.
   - **SkipDeleted** → do not resurrect; report it.
3. Return an `ImportReport` (imported / updated / preserved / skipped) and the updated manifest for the caller to persist.

Use **`sha2`** (SHA-256) as the manifest content hash. It is stable across runs, platforms, and versions, which a `std` hasher is not, so it is safe to persist and compare.

## Consequences

- The backend import works end to end and is verified by scenario tests against an in-test database and a temporary vault: fresh import, idempotent re-import, **re-import preserves a user-edited file**, re-import does not resurrect a deleted file, and locked notes are skipped.
- `sha2` is a new dependency (small, pure Rust, RustCrypto). It fulfils the content-hash placeholder from ADR-0137.
- Known v1 limitations, documented for later slices: a new note whose slug collides with a pre-existing non-import file is reported (never overwritten) rather than auto-renamed; a note retitled in Apple keeps its original filename rather than being renamed; folder hierarchy, attachments, and tables are not yet placed. The Tauri command, the Full Disk Access flow, and the UI wrap this function and require native QA.
