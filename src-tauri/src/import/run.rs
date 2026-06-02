//! Run a full Apple Notes import: read the store, write notes into the vault.
//!
//! This is the orchestration that ties the import modules together. It reads
//! every note from a copied `NoteStore.sqlite`, assembles each into markdown,
//! and writes it into the destination directory, reconciling against a prior
//! [`ImportManifest`] so a re-run is idempotent and never overwrites a note the
//! user edited after a previous import.
//!
//! It is deliberately free of any Tauri / Full Disk Access concerns: it takes
//! plain directories, so it is fully testable against an in-test database and a
//! temporary vault. The Tauri command and the permission flow wrap this.

use std::collections::HashSet;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::import::assemble::{assemble_note, AssembledNote};
use crate::import::manifest::{ImportManifest, ManifestEntry, ReimportDecision};
use crate::import::materialize;
use crate::import::store::{self, RawNote};
use crate::vault::save_note_content;

/// Manifest source identifier for Apple Notes imports.
pub const SOURCE: &str = "apple-notes";

/// One note that was not imported, with a human-readable reason.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SkippedNote {
    pub source_id: String,
    pub reason: String,
}

/// The outcome of an import run.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ImportReport {
    /// Notes written for the first time.
    pub imported: usize,
    /// Previously-imported, untouched notes that were rewritten.
    pub updated: usize,
    /// Destination paths left untouched because the user edited them.
    pub preserved: Vec<String>,
    /// Notes that were not imported (locked, undecodable, deleted-after-import).
    pub skipped: Vec<SkippedNote>,
}

/// Import every note from `notes_dir`'s `NoteStore.sqlite` into `dest_dir`,
/// reconciling against `prior`. Returns the report and the updated manifest for
/// the caller to persist.
pub fn run_import(
    notes_dir: &Path,
    dest_dir: &Path,
    prior: &ImportManifest,
) -> Result<(ImportReport, ImportManifest), String> {
    let work = tempfile::tempdir().map_err(|err| format!("failed to create temp dir: {err}"))?;
    let db = store::copy_database(notes_dir, work.path())?;
    let conn = store::open_checkpointed(&db)?;
    let raws = store::enumerate_notes(&conn)?;

    let mut ctx = ImportContext::new(prior);
    for raw in &raws {
        ctx.process(raw, dest_dir)?;
    }
    Ok((ctx.report, ctx.manifest))
}

struct ImportContext<'a> {
    prior: &'a ImportManifest,
    manifest: ImportManifest,
    report: ImportReport,
    taken: HashSet<String>,
}

impl<'a> ImportContext<'a> {
    fn new(prior: &'a ImportManifest) -> Self {
        // Seed taken stems with the prior run's paths so a newly-added note
        // never collides with a note we already placed.
        let taken = prior.entries.values().map(|e| stem_of(&e.dest_path)).collect();
        Self {
            prior,
            manifest: ImportManifest::new(SOURCE),
            report: ImportReport::default(),
            taken,
        }
    }

    fn process(&mut self, raw: &RawNote, dest_dir: &Path) -> Result<(), String> {
        if raw.password_protected {
            self.skip(raw, "password protected");
            return Ok(());
        }
        let assembled = match assemble_note(raw) {
            Ok(note) => note,
            Err(reason) => {
                self.skip(raw, &reason);
                return Ok(());
            }
        };

        let rel_path = self.dest_path_for(&assembled);
        let abs_path = dest_dir.join(&rel_path);
        let new_hash = sha256_hex(&assembled.markdown);
        let on_disk = read_hash_if_exists(&abs_path);

        match self.prior.decide(&raw.source_id, on_disk.as_deref()) {
            ReimportDecision::SkipDeleted => self.skip(raw, "deleted after a prior import"),
            ReimportDecision::PreserveUserEdit => self.preserve(raw, rel_path),
            ReimportDecision::Create => self.write(&abs_path, &rel_path, &assembled, new_hash, false)?,
            ReimportDecision::Update => self.write(&abs_path, &rel_path, &assembled, new_hash, true)?,
        }
        Ok(())
    }

    /// A previously-imported note keeps its recorded path (so a title change does
    /// not orphan the old file or trip the deletion check); a new note gets a
    /// fresh slug.
    fn dest_path_for(&mut self, assembled: &AssembledNote) -> String {
        if let Some(entry) = self.prior.entry(&assembled.source_id) {
            return entry.dest_path.clone();
        }
        format!("{}.md", materialize::unique_stem(&assembled.title, &mut self.taken))
    }

    fn write(
        &mut self,
        abs: &Path,
        rel: &str,
        note: &AssembledNote,
        hash: String,
        update: bool,
    ) -> Result<(), String> {
        let abs_str = abs
            .to_str()
            .ok_or_else(|| format!("non-UTF-8 destination path: {}", abs.display()))?;
        save_note_content(abs_str, &note.markdown)?;
        apply_modified_time(abs, note.modified_unix);
        self.manifest.upsert(ManifestEntry {
            source_id: note.source_id.clone(),
            dest_path: rel.to_string(),
            content_hash: hash,
            attachment_paths: Vec::new(),
            imported_at: now_unix(),
        });
        if update {
            self.report.updated += 1;
        } else {
            self.report.imported += 1;
        }
        Ok(())
    }

    fn preserve(&mut self, raw: &RawNote, rel: String) {
        self.report.preserved.push(rel);
        self.carry_forward(raw);
    }

    fn skip(&mut self, raw: &RawNote, reason: &str) {
        self.report.skipped.push(SkippedNote {
            source_id: raw.source_id.clone(),
            reason: reason.to_string(),
        });
        self.carry_forward(raw);
    }

    /// Keep a prior manifest entry so re-running does not lose its history.
    fn carry_forward(&mut self, raw: &RawNote) {
        if let Some(entry) = self.prior.entry(&raw.source_id) {
            self.manifest.upsert(entry.clone());
        }
    }
}

fn stem_of(dest_path: &str) -> String {
    dest_path.strip_suffix(".md").unwrap_or(dest_path).to_string()
}

fn sha256_hex(content: &str) -> String {
    let digest = Sha256::digest(content.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn read_hash_if_exists(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok().map(|c| sha256_hex(&c))
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Apply the note's modified time to the written file (best-effort). Tolaria
/// sources note dates from the filesystem, so this is how a migrated note keeps
/// its Apple modification date.
fn apply_modified_time(path: &Path, modified_unix: Option<f64>) {
    let Some(secs) = modified_unix else { return };
    if secs < 0.0 {
        return;
    }
    let time = UNIX_EPOCH + Duration::from_secs_f64(secs);
    if let Ok(file) = std::fs::OpenOptions::new().write(true).open(path) {
        let _ = file.set_modified(time);
    }
}

#[cfg(test)]
mod tests {
    use super::{run_import, ImportManifest};
    use crate::import::body::encode_plain_note_gzip;
    use rusqlite::Connection;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    /// Build a NoteStore.sqlite in `dir` from `(source_id, title, body, locked)`.
    fn make_store(dir: &Path, notes: &[(&str, &str, &str, bool)]) {
        let conn = Connection::open(dir.join("NoteStore.sqlite")).unwrap();
        conn.execute_batch(
            "CREATE TABLE ZICCLOUDSYNCINGOBJECT (
                 Z_PK INTEGER PRIMARY KEY, ZIDENTIFIER TEXT, ZTITLE1 TEXT,
                 ZCREATIONDATE REAL, ZMODIFIEDDATE1 REAL, ZISPASSWORDPROTECTED INTEGER, ZNOTE INTEGER);
             CREATE TABLE ZICNOTEDATA (Z_PK INTEGER PRIMARY KEY, ZNOTE INTEGER, ZDATA BLOB);",
        )
        .unwrap();
        for (index, (source_id, title, body, locked)) in notes.iter().enumerate() {
            let pk = (index + 1) as i64;
            conn.execute(
                "INSERT INTO ZICCLOUDSYNCINGOBJECT
                    (Z_PK, ZIDENTIFIER, ZTITLE1, ZMODIFIEDDATE1, ZISPASSWORDPROTECTED)
                 VALUES (?1, ?2, ?3, 700000000.0, ?4)",
                rusqlite::params![pk, source_id, title, i64::from(*locked)],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO ZICNOTEDATA (Z_PK, ZNOTE, ZDATA) VALUES (?1, ?1, ?2)",
                rusqlite::params![pk, encode_plain_note_gzip(body)],
            )
            .unwrap();
        }
    }

    #[test]
    fn fresh_import_writes_notes() {
        let src = TempDir::new().unwrap();
        make_store(src.path(), &[("a", "First Note", "alpha", false), ("b", "Second", "beta", false)]);
        let vault = TempDir::new().unwrap();

        let (report, manifest) =
            run_import(src.path(), vault.path(), &ImportManifest::new("apple-notes")).unwrap();

        assert_eq!(report.imported, 2);
        assert_eq!(report.updated, 0);
        assert_eq!(fs::read_to_string(vault.path().join("first-note.md")).unwrap(), "alpha");
        assert_eq!(fs::read_to_string(vault.path().join("second.md")).unwrap(), "beta");
        assert_eq!(manifest.entries.len(), 2);
    }

    #[test]
    fn reimport_unchanged_is_idempotent() {
        let src = TempDir::new().unwrap();
        make_store(src.path(), &[("a", "Note", "body", false)]);
        let vault = TempDir::new().unwrap();

        let (_, manifest) =
            run_import(src.path(), vault.path(), &ImportManifest::new("apple-notes")).unwrap();
        let (report, _) = run_import(src.path(), vault.path(), &manifest).unwrap();

        assert_eq!(report.imported, 0);
        assert_eq!(report.updated, 1);
        assert_eq!(report.preserved.len(), 0);
        // Only one file exists; no duplicate.
        let count = fs::read_dir(vault.path()).unwrap().count();
        assert_eq!(count, 1);
    }

    #[test]
    fn reimport_preserves_user_edits() {
        let src = TempDir::new().unwrap();
        make_store(src.path(), &[("a", "Note", "original", false)]);
        let vault = TempDir::new().unwrap();

        let (_, manifest) =
            run_import(src.path(), vault.path(), &ImportManifest::new("apple-notes")).unwrap();
        let note_path = vault.path().join("note.md");
        fs::write(&note_path, "MY EDIT").unwrap();

        let (report, _) = run_import(src.path(), vault.path(), &manifest).unwrap();

        assert_eq!(report.updated, 0);
        assert_eq!(report.preserved, vec!["note.md".to_string()]);
        assert_eq!(fs::read_to_string(&note_path).unwrap(), "MY EDIT");
    }

    #[test]
    fn reimport_does_not_resurrect_deleted() {
        let src = TempDir::new().unwrap();
        make_store(src.path(), &[("a", "Note", "body", false)]);
        let vault = TempDir::new().unwrap();

        let (_, manifest) =
            run_import(src.path(), vault.path(), &ImportManifest::new("apple-notes")).unwrap();
        fs::remove_file(vault.path().join("note.md")).unwrap();

        let (report, _) = run_import(src.path(), vault.path(), &manifest).unwrap();

        assert_eq!(report.imported, 0);
        assert_eq!(report.updated, 0);
        assert!(!vault.path().join("note.md").exists());
        assert_eq!(report.skipped.len(), 1);
        assert!(report.skipped[0].reason.contains("deleted"));
    }

    #[test]
    fn password_protected_notes_are_skipped() {
        let src = TempDir::new().unwrap();
        make_store(src.path(), &[("a", "Open", "visible", false), ("b", "Locked", "secret", true)]);
        let vault = TempDir::new().unwrap();

        let (report, _) =
            run_import(src.path(), vault.path(), &ImportManifest::new("apple-notes")).unwrap();

        assert_eq!(report.imported, 1);
        assert_eq!(report.skipped.len(), 1);
        assert_eq!(report.skipped[0].source_id, "b");
        assert!(report.skipped[0].reason.contains("password"));
        assert!(vault.path().join("open.md").exists());
        assert!(!vault.path().join("locked.md").exists());
    }
}
