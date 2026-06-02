//! Foundations for importing external note stores into the vault.
//!
//! This module hosts source-agnostic building blocks shared by concrete
//! importers (Apple Notes first; Notion/Bear/Evernote later). The first piece
//! is the import [`manifest`], which records what each import run wrote so that
//! re-running an import is idempotent, resumable, and — crucially — never
//! overwrites a note the user edited in Tolaria after a prior import.
//!
//! Decode-specific work (SQLite access, gzip, protobuf) and filename
//! materialization land in later slices, behind their own ADRs and reusing the
//! existing `vault` machinery (`filename_rules`, `rename`, `title_sync`).

pub mod body;
pub mod convert;
pub mod manifest;
pub mod materialize;
