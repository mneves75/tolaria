# ADR-0137: Import Run Manifest and Re-Import Conflict Policy

## Status

Accepted

## Context

Tolaria will gain importers for external note stores, starting with Apple Notes. Importing hundreds-to-thousands of notes is not a one-shot operation: users re-run it after adding notes in the source app, an import can be interrupted on a large library, and a user may edit an imported note inside Tolaria before re-importing.

Without a record of what an import wrote, every one of these becomes unsafe. A naive re-import either duplicates notes, silently overwrites edits the user made after the first import, or resurrects notes the user deleted. For a product whose first principle is "the filesystem is the single source of truth," destroying a user's edits is the least forgivable failure.

These needs — idempotency, resume, undo, and conflict-safe re-import — are not four features. They are one artifact: a manifest of what each run wrote.

## Decision

Introduce a source-agnostic `import` module (`src-tauri/src/import/`) whose first component is an **import manifest** (`import::manifest`).

- `ImportManifest` records, per source note keyed by a stable `source_id` (e.g. the Apple Notes `ZIDENTIFIER`), the destination path, an opaque content hash of what the import wrote, the attachment paths it copied, and a timestamp. It is `serde`-serializable to a vault-side JSON file so it follows the vault across installs, consistent with the filesystem-as-truth principle.
- Re-import policy is a pure function, `resolve_reimport(prior, on_disk_hash) -> ReimportDecision`:
  - never imported, no file present → `Create`
  - never imported, but a file already occupies the path → `PreserveUserEdit` (the file is the user's; the caller routes to an alternate name)
  - imported before, file now absent → `SkipDeleted` (the user deleted it; do not resurrect)
  - imported before, on-disk hash equals the recorded hash → `Update` (untouched; safe to rewrite)
  - imported before, on-disk hash differs → `PreserveUserEdit` (the user edited it; never clobber)

Content hashes are treated as opaque strings. Computing them over the materialized markdown belongs to the importer slice that writes notes; keeping the comparison hash-agnostic makes the policy pure and exhaustively testable without a filesystem or a hashing dependency.

## Consequences

- Re-import never overwrites a user-edited note and never resurrects a deleted one; these guarantees are unit-tested over every input combination.
- The manifest gives later slices what they need for free: resume (skip entries already written), dedup (look up by `source_id`), and undo (the manifest lists exactly the files and attachments a run created).
- This ADR covers only the manifest seam and the re-import policy. Source decoding (SQLite access, gzip, protobuf), the new Rust dependencies it requires, the Full Disk Access flow, and filename materialization (which must reuse `vault::filename_rules`, `vault::rename`, and `vault::title_sync` rather than duplicate them) are deferred to their own ADRs and commits.
- The content-hash function is intentionally absent here; the slice that introduces it will pick a stable hash (e.g. SHA-256) and record that dependency choice in its own ADR.
