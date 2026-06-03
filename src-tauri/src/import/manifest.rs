//! Import run manifest and re-import conflict resolution.
//!
//! An import manifest records, per source note, where the import wrote it and a
//! hash of the content it wrote. On a later run the manifest answers the only
//! questions that keep re-import safe:
//!
//! - Is this note new, or did we import it before?
//! - Did the user edit our imported file afterwards (so we must not clobber it)?
//! - Did the user delete it (so we must not resurrect it)?
//!
//! The manifest treats content hashes as opaque strings. Computing them (over
//! the materialized markdown) belongs to the importer slice that writes notes;
//! keeping the comparison logic hash-agnostic makes it pure and fully testable.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Schema version for the on-disk manifest file.
pub const CURRENT_MANIFEST_VERSION: u32 = 1;

/// One imported note's bookkeeping from the most recent import run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestEntry {
    /// Stable identifier from the source store (e.g. an Apple Notes ZIDENTIFIER).
    pub source_id: String,
    /// Vault-relative path of the markdown file this import wrote.
    pub dest_path: String,
    /// Opaque hash of the markdown content this import last wrote to `dest_path`.
    /// Comparing it against the file's current hash detects post-import edits.
    pub content_hash: String,
    /// Vault-relative paths of attachments copied for this note.
    #[serde(default)]
    pub attachment_paths: Vec<String>,
    /// Unix seconds when this entry was last written by an import run.
    pub imported_at: u64,
}

/// A complete record of one import source's prior runs, keyed by `source_id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportManifest {
    /// Schema version of this manifest.
    pub version: u32,
    /// Identifier of the import source (e.g. `"apple-notes"`).
    pub source: String,
    /// Entries keyed by their `source_id`.
    pub entries: BTreeMap<String, ManifestEntry>,
}

/// What a re-import should do with a single source note.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReimportDecision {
    /// Never imported and no file in the way: write it fresh.
    Create,
    /// Imported before and the file is untouched: safe to rewrite.
    Update,
    /// A file exists that the user owns or edited: do not overwrite it.
    PreserveUserEdit,
    /// Imported before but the user has since deleted it: do not resurrect.
    SkipDeleted,
}

impl ImportManifest {
    /// Create an empty manifest for `source` at the current schema version.
    pub fn new(source: impl Into<String>) -> Self {
        Self {
            version: CURRENT_MANIFEST_VERSION,
            source: source.into(),
            entries: BTreeMap::new(),
        }
    }

    /// Look up the entry recorded for `source_id`, if any.
    pub fn entry(&self, source_id: &str) -> Option<&ManifestEntry> {
        self.entries.get(source_id)
    }

    /// Insert or replace the entry for its `source_id`.
    pub fn upsert(&mut self, entry: ManifestEntry) {
        self.entries.insert(entry.source_id.clone(), entry);
    }

    /// Decide how to handle a source note on re-import.
    ///
    /// `on_disk_hash` is the current hash of the file at the recorded
    /// destination, or `None` when no file exists there now.
    pub fn decide(&self, source_id: &str, on_disk_hash: Option<&str>) -> ReimportDecision {
        resolve_reimport(self.entry(source_id), on_disk_hash)
    }

    /// Load the manifest at `path`, or return a fresh one for `source` if the
    /// file does not exist yet (the first import).
    pub fn load_or_new(path: &Path, source: &str) -> Result<Self, String> {
        if !path.exists() {
            return Ok(Self::new(source));
        }
        let json = std::fs::read_to_string(path)
            .map_err(|err| format!("failed to read manifest {}: {err}", path.display()))?;
        serde_json::from_str(&json)
            .map_err(|err| format!("failed to parse manifest {}: {err}", path.display()))
    }

    /// Write the manifest to `path`, creating parent directories as needed.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|err| format!("failed to create {}: {err}", parent.display()))?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|err| format!("failed to serialize manifest: {err}"))?;
        std::fs::write(path, json)
            .map_err(|err| format!("failed to write manifest {}: {err}", path.display()))
    }
}

/// Pure re-import decision from a prior entry and the file's current hash.
///
/// `prior` is the manifest entry from the last run (`None` if never imported).
/// `on_disk_hash` is the file's current content hash (`None` if absent).
pub fn resolve_reimport(
    prior: Option<&ManifestEntry>,
    on_disk_hash: Option<&str>,
) -> ReimportDecision {
    match (prior, on_disk_hash) {
        // Never imported, nothing in the way: fresh write.
        (None, None) => ReimportDecision::Create,
        // Never imported, but a file already occupies the path: it is the
        // user's, not ours. Leave it; the caller routes to an alternate name.
        (None, Some(_)) => ReimportDecision::PreserveUserEdit,
        // Imported before, file now gone: the user deleted it. Respect that.
        (Some(_), None) => ReimportDecision::SkipDeleted,
        // Imported before and still present: update only if untouched.
        (Some(entry), Some(current)) => {
            if entry.content_hash == current {
                ReimportDecision::Update
            } else {
                ReimportDecision::PreserveUserEdit
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        resolve_reimport, ImportManifest, ManifestEntry, ReimportDecision, CURRENT_MANIFEST_VERSION,
    };

    fn entry(source_id: &str, hash: &str) -> ManifestEntry {
        ManifestEntry {
            source_id: source_id.to_string(),
            dest_path: format!("Imported/{source_id}.md"),
            content_hash: hash.to_string(),
            attachment_paths: Vec::new(),
            imported_at: 1_700_000_000,
        }
    }

    #[test]
    fn fresh_note_with_no_file_is_created() {
        assert_eq!(resolve_reimport(None, None), ReimportDecision::Create);
    }

    #[test]
    fn unknown_note_colliding_with_existing_file_preserves_it() {
        assert_eq!(
            resolve_reimport(None, Some("anything")),
            ReimportDecision::PreserveUserEdit
        );
    }

    #[test]
    fn previously_imported_then_deleted_is_not_resurrected() {
        let prior = entry("apple-1", "hash-a");
        assert_eq!(
            resolve_reimport(Some(&prior), None),
            ReimportDecision::SkipDeleted
        );
    }

    #[test]
    fn unedited_imported_note_is_updated() {
        let prior = entry("apple-1", "hash-a");
        assert_eq!(
            resolve_reimport(Some(&prior), Some("hash-a")),
            ReimportDecision::Update
        );
    }

    #[test]
    fn user_edited_imported_note_is_preserved() {
        let prior = entry("apple-1", "hash-a");
        assert_eq!(
            resolve_reimport(Some(&prior), Some("hash-b-user-edit")),
            ReimportDecision::PreserveUserEdit
        );
    }

    #[test]
    fn decide_uses_recorded_entry() {
        let mut manifest = ImportManifest::new("apple-notes");
        manifest.upsert(entry("apple-1", "hash-a"));

        assert_eq!(
            manifest.decide("apple-1", Some("hash-a")),
            ReimportDecision::Update
        );
        assert_eq!(
            manifest.decide("apple-1", Some("edited")),
            ReimportDecision::PreserveUserEdit
        );
        assert_eq!(
            manifest.decide("apple-1", None),
            ReimportDecision::SkipDeleted
        );
        assert_eq!(manifest.decide("unknown", None), ReimportDecision::Create);
    }

    #[test]
    fn new_manifest_starts_empty_and_versioned() {
        let manifest = ImportManifest::new("apple-notes");
        assert_eq!(manifest.version, CURRENT_MANIFEST_VERSION);
        assert_eq!(manifest.source, "apple-notes");
        assert!(manifest.entries.is_empty());
        assert!(manifest.entry("apple-1").is_none());
    }

    #[test]
    fn upsert_inserts_then_replaces() {
        let mut manifest = ImportManifest::new("apple-notes");
        manifest.upsert(entry("apple-1", "hash-a"));
        assert_eq!(manifest.entry("apple-1").unwrap().content_hash, "hash-a");

        manifest.upsert(entry("apple-1", "hash-b"));
        assert_eq!(manifest.entries.len(), 1);
        assert_eq!(manifest.entry("apple-1").unwrap().content_hash, "hash-b");
    }

    #[test]
    fn manifest_round_trips_through_json() {
        let mut manifest = ImportManifest::new("apple-notes");
        let mut populated = entry("apple-1", "hash-a");
        populated.attachment_paths = vec!["attachments/img-1.png".to_string()];
        manifest.upsert(populated);

        let json = serde_json::to_string(&manifest).expect("serialize");
        let restored: ImportManifest = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(manifest, restored);
    }

    #[test]
    fn entry_deserializes_without_attachments_field() {
        let json = r#"{
            "source_id": "apple-1",
            "dest_path": "Imported/apple-1.md",
            "content_hash": "hash-a",
            "imported_at": 1700000000
        }"#;

        let entry: ManifestEntry = serde_json::from_str(json).expect("deserialize");
        assert!(entry.attachment_paths.is_empty());
    }

    #[test]
    fn load_or_new_returns_fresh_when_file_absent() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("nested").join("manifest.json");
        let manifest = ImportManifest::load_or_new(&path, "apple-notes").unwrap();
        assert_eq!(manifest, ImportManifest::new("apple-notes"));
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("sub").join("manifest.json");
        let mut manifest = ImportManifest::new("apple-notes");
        manifest.upsert(entry("apple-1", "hash-a"));

        manifest.save(&path).expect("save");
        assert!(path.exists());
        let loaded = ImportManifest::load_or_new(&path, "apple-notes").unwrap();
        assert_eq!(manifest, loaded);
    }

    #[test]
    fn load_or_new_errors_on_malformed_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("manifest.json");
        std::fs::write(&path, "{ not valid json").unwrap();
        let err = ImportManifest::load_or_new(&path, "apple-notes").unwrap_err();
        assert!(err.contains("parse manifest"), "unexpected error: {err}");
    }
}
