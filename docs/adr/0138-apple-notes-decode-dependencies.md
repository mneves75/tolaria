# ADR-0138: Apple Notes Decode Dependencies and Vendored Protobuf Schema

## Status

Accepted

## Context

Apple Notes stores each note body in `ZICNOTEDATA.ZDATA` as a gzip-framed protobuf. Reconstructing a note therefore requires three capabilities Tolaria did not previously have in Rust: gzip decompression, protobuf decoding, and a schema for Apple's undocumented note format.

The note format is not published by Apple. The de-facto reference is the schema reverse-engineered by Three Planets Software in `apple_cloud_notes_parser`; the official Obsidian importer vendors the same schema. Re-deriving it from scratch would be weeks of work for no benefit.

## Decision

Add three capabilities, scoped to the importer:

- **Decompression:** `flate2` (default `miniz_oxide` backend, pure Rust). Note bodies are gzip-framed (magic `1f 8b`), so `flate2::read::GzDecoder` is the correct decoder.
- **Protobuf:** `prost` (runtime) with build-time codegen. The schema is compiled by **`protox`** (a pure-Rust protobuf compiler) feeding `prost-build`, rather than shelling out to `protoc`. This keeps the build self-contained: no `protoc` system dependency for CI or contributors.
- **Schema:** vendor the note-body messages of `apple_cloud_notes_parser`'s `notestore.proto` into `src-tauri/proto/notestore.proto`, verbatim field numbers, under an MIT attribution header (the schema is MIT, © 2019 Three Planets Software; this is the same approach Obsidian uses). The embedded-object / mergeable-data (CRDT) messages used for tables are intentionally omitted for now and will be added with the table-import slice, because several of their `required` fields do not hold in real data and would make `prost` decoding brittle.

`build.rs` compiles the proto via `protox` + `prost-build`; the body module (`import::body`) gunzips, decodes `NoteStoreProto`, and maps the protobuf formatting runs onto the protobuf-free `import::convert` domain types.

## Consequences

- Tolaria can turn a real note-body blob into markdown; the gzip → protobuf → domain → markdown path is covered by unit tests that build, encode, and gzip protobuf messages in-test (no `protoc`, no SQLite, no real Apple data required).
- `flate2`, `prost`, `prost-build`, and `protox` are new dependencies. They are widely used, actively maintained, and pure Rust (no `protoc`); the build gains a protobuf codegen step in `build.rs`.
- The vendored schema is a subset of upstream. Adding the CRDT/table messages later will require relaxing some upstream `required` fields to `optional` in our vendored copy (a documented deviation), because `prost` rejects missing `required` fields that Apple does not always populate.
- Coverage note: prost generates code under `OUT_DIR`. If line-coverage gating counts it, exclude the generated file via an ignore pattern; the body module's own logic is fully tested.
